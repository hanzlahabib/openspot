use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
use openspot_wlan::{Adapter, WlanEvent, WlanProvider};

use crate::adapter_watch::AdapterWatch;
use crate::open_arg;
use crate::aliases::{self, AliasScope, Aliases};
use crate::inspector::{AdapterChange, Inspector};
use crate::planning::{self, Planning};
use crate::prefs::{self, Prefs, UpdatePrefs};
use crate::sound::SoundNotifier;
use crate::survey::{self, Survey};
use crate::update_check::{UpdateChecker, UpdateStatus};
use crate::theme::Appearance;
use crate::ui::prefs_window::{PrefsContext, PrefsWindow};
use crate::worker::{Command, Event, Worker};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Inspector,
    Survey,
    Planning,
}

pub struct OpenSpotApp {
    start: Instant,
    worker: Worker,
    events: Receiver<Event>,
    mode: Mode,
    inspector: Inspector,
    survey: Survey,
    planning: Planning,
    adapter_watch: AdapterWatch,
    aliases: Aliases,
    alias_scope: AliasScope,
    appearance: Appearance,
    /// Appearance last pushed to egui, so it is only re-applied on change.
    applied_appearance: Option<Appearance>,
    prefs_window: PrefsWindow,
    sound: SoundNotifier,
    updates: UpdatePrefs,
    update_checker: UpdateChecker,
    startup_check_done: bool,
    prefs_path: Option<PathBuf>,
    /// What is on disk now, what the UI currently holds, and when the latter last changed.
    saved_prefs: Prefs,
    live_prefs: Prefs,
    prefs_changed_at: Option<Instant>,
    prefs_notice: Option<String>,
}

/// Wait this long after the last change before writing: dragging a slider changes the value
/// every frame, and one write per settled change is plenty.
const PREFS_SAVE_DELAY: Duration = Duration::from_millis(600);

impl OpenSpotApp {
    pub fn new(ctx: &egui::Context, provider: Box<dyn WlanProvider>, adapters: Vec<Adapter>, initial_project: Option<&std::path::Path>) -> Self {
        let prefs_path = prefs::default_path();
        let loaded = prefs_path.as_deref().map(prefs::load_from);
        let (stored, prefs_notice) = match loaded {
            Some(l) => (l.prefs, l.warning),
            None => (Prefs::default(), None),
        };
        openspot_i18n::set_preference(stored.language.as_deref());
        let mut inspector = Inspector::new(adapters.clone());
        inspector.apply_prefs(&stored.inspector);
        let aliases = Aliases::from_map(&stored.aliases);
        let mut survey = Survey::default();
        survey.apply_prefs(&stored.survey);
        survey.report.branding = stored.report.clone();
        let planning = Planning::new(&stored.planning);

        // A project given on the command line (double-click / file association) opens in Survey.
        // A failure is shown in the mode bar; the app still starts normally.
        let mut prefs_notice = prefs_notice;
        let mut mode = Mode::Inspector;
        if let Some(path) = initial_project {
            match open_arg::resolve(path).and_then(|db| survey.open_from(&db)) {
                Ok(()) => mode = Mode::Survey,
                Err(e) => prefs_notice = Some(e),
            }
        }

        let repaint = ctx.clone();
        let (worker, events) =
            Worker::spawn(provider, adapters[0].clone(), inspector.interval(), move || repaint.request_repaint());
        if mode == Mode::Survey {
            // Same as switching to Survey by hand: background Inspector scans would compete with
            // survey measurements for the radio.
            worker.send(Command::SetPaused(true));
        }
        Self {
            start: Instant::now(),
            worker,
            events,
            mode,
            inspector,
            survey,
            planning,
            adapter_watch: AdapterWatch::default(),
            aliases,
            alias_scope: stored.alias_scope,
            appearance: stored.appearance.sanitized(),
            applied_appearance: None,
            prefs_window: PrefsWindow::default(),
            sound: SoundNotifier::system(),
            updates: stored.updates.clone(),
            update_checker: UpdateChecker::default(),
            startup_check_done: false,
            prefs_path,
            live_prefs: stored.clone(),
            saved_prefs: stored,
            prefs_changed_at: None,
            prefs_notice,
        }
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn drain_events(&mut self) {
        let now = self.now();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Scan(aps) => {
                    if let Some(cue) = self.inspector.ingest(aps, now)
                        && let Some(problem) = self.sound.play(cue)
                    {
                        self.inspector.set_notice(problem);
                    }
                }
                Event::Error(message) => self.inspector.set_error(message, now),
                Event::SurveyScan { x, y, result, speed, link } => {
                    self.survey.set_scan_link(link);
                    self.survey.on_scan_result(x, y, result, speed);
                    if let Some(cue) = self.survey.take_cue()
                        && let Some(problem) = self.sound.play(cue)
                    {
                        self.survey.message = Some(problem);
                    }
                }
                Event::ActiveProgress(message) => self.survey.active.progress = Some(message),
                Event::Hardware { event, adapters } => self.on_hardware(&event, adapters, now),
            }
        }
        self.inspector.housekeeping(now);
    }

    /// Applies an adapter hot-plug or radio notification (S-05).
    fn on_hardware(&mut self, event: &WlanEvent, listing: Option<Vec<Adapter>>, now: f64) {
        let update = self.adapter_watch.apply(self.inspector.adapters(), event, listing);
        if let Some(list) = update.adapters {
            match self.inspector.set_adapters(list, now) {
                AdapterChange::Unchanged => {}
                AdapterChange::Switched(adapter) => {
                    self.worker.send(Command::SetAdapter(adapter));
                    self.worker.send(Command::SetPaused(self.scan_paused()));
                }
                AdapterChange::Lost => self.worker.send(Command::SetPaused(self.scan_paused())),
            }
        }
        if let Some(notice) = update.notice {
            self.survey.message = Some(notice.clone());
            self.inspector.set_notice(notice);
        }
    }

    /// Whether background scanning should be off: Survey owns the radio, the user paused it,
    /// or there is no adapter to scan on.
    fn scan_paused(&self) -> bool {
        self.mode == Mode::Survey || self.inspector.paused || self.inspector.active_adapter().is_none()
    }

    fn current_prefs(&self) -> Prefs {
        Prefs {
            inspector: self.inspector.prefs(),
            survey: self.survey.prefs(),
            appearance: self.appearance,
            updates: self.updates.clone(),
            aliases: self.aliases.to_map(),
            alias_scope: self.alias_scope,
            report: self.survey.report.branding.clone(),
            language: openspot_i18n::preference(),
            planning: self.planning.prefs(),
            ..Prefs::default()
        }
    }

    /// Writes preferences once they have been stable for [`PREFS_SAVE_DELAY`].
    fn persist_prefs(&mut self, ctx: &egui::Context, force: bool) {
        let current = self.current_prefs();
        if current != self.live_prefs {
            self.live_prefs = current;
            self.prefs_changed_at = Some(Instant::now());
        }
        let Some(changed_at) = self.prefs_changed_at else { return };
        if !force && changed_at.elapsed() < PREFS_SAVE_DELAY {
            ctx.request_repaint_after(PREFS_SAVE_DELAY);
            return;
        }
        self.prefs_changed_at = None;
        if self.live_prefs == self.saved_prefs {
            return;
        }
        if let Some(path) = &self.prefs_path {
            match prefs::save_to(path, &self.live_prefs) {
                Ok(()) => self.saved_prefs = self.live_prefs.clone(),
                Err(e) => self.prefs_notice = Some(format!("could not save preferences: {e}")),
            }
        }
    }

    fn switch_mode(&mut self, mode: Mode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        // Background Inspector scans would compete with survey measurements for the radio.
        self.worker.send(Command::SetPaused(self.scan_paused()));
    }
}

impl eframe::App for OpenSpotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.applied_appearance != Some(self.appearance) {
            self.appearance.apply(ctx);
            self.applied_appearance = Some(self.appearance);
        }
        if self.updates.check_on_startup && !self.startup_check_done {
            self.startup_check_done = true;
            self.update_checker.start(ctx);
        }
        if self.update_checker.poll()
            && let Some(Ok(UpdateStatus::Available { version, .. })) = &self.update_checker.result
        {
            self.inspector.set_notice(format!("OpenSpot {version} is available (see Preferences, Updates)"));
        }
        self.drain_events();

        let mut mode = self.mode;
        egui::TopBottomPanel::top("mode").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut mode, Mode::Inspector, openspot_i18n::tr("mode-inspector"));
                ui.selectable_value(&mut mode, Mode::Survey, openspot_i18n::tr("mode-survey"));
                ui.selectable_value(&mut mode, Mode::Planning, openspot_i18n::tr("mode-planning"));
                ui.separator();
                if ui.button(openspot_i18n::tr("mode-preferences")).clicked() {
                    self.prefs_window.open = true;
                }
                if let Some(notice) = &self.prefs_notice {
                    ui.separator();
                    ui.colored_label(egui::Color32::LIGHT_RED, notice);
                }
            });
        });
        self.switch_mode(mode);
        if let Some(banner) = self.adapter_watch.banner(self.inspector.active_adapter()) {
            egui::TopBottomPanel::top("adapter_banner").show(ctx, |ui| {
                ui.colored_label(egui::Color32::YELLOW, banner);
            });
        }

        // The UI edits one working copy of the active aliases; it is stored back below.
        let (scope, project_id) = (self.alias_scope, self.survey.project.id);
        let before = aliases::active(scope, &self.aliases, &self.survey.project.aliases);
        let mut aliases = before.clone();
        match self.mode {
            Mode::Inspector => self.inspector.show(ctx, &self.worker, self.now(), &mut aliases),
            Mode::Survey => survey::show(&mut self.survey, ctx, &self.worker, &mut aliases, self.inspector.groups()),
            Mode::Planning => planning::show(&mut self.planning, &mut self.survey, ctx),
        }
        self.inspector.floating_windows(ctx, &mut aliases);
        let now = self.now();
        let mut pc = PrefsContext {
            inspector: &mut self.inspector,
            survey: &mut self.survey,
            aliases: &aliases,
            alias_scope: &mut self.alias_scope,
            appearance: &mut self.appearance,
            sound: &mut self.sound,
            updates: &mut self.updates,
            checker: &mut self.update_checker,
            worker: &self.worker,
            now,
        };
        self.prefs_window.show(ctx, &mut pc);
        // A project opened this frame has its own aliases; an edit made against the previous
        // project's copy must not be written into it.
        if aliases != before && (scope == AliasScope::App || project_id == self.survey.project.id) {
            let changed = aliases::commit(scope, aliases, &mut self.aliases, &mut self.survey.project.aliases);
            if changed {
                self.survey.mark_dirty();
            }
        }
        self.persist_prefs(ctx, false);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_prefs(&egui::Context::default(), true);
    }
}
