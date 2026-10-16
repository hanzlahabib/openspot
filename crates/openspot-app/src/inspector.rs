use std::collections::BTreeSet;
use std::time::Duration;

use eframe::egui;
use openspot_wlan::{AccessPoint, Adapter, Band, MacAddress};

use crate::aliases::Aliases;
use crate::columns::{Column, ColumnSet};
use crate::event_log::{ERASE_PRESETS_SECS, EventFilter, EventKind, EventLog};
use crate::sound::Cue;
use crate::grouping::GroupBy;
use crate::adapter_watch::{ActiveAdapter, choose_active};
use crate::groups::CustomGroups;
use crate::prefs::InspectorPrefs;
use crate::state::{Filter, Sort, Tracker};
use crate::table_export;
use crate::ui::alias_io::AliasIo;
use crate::ui::groups::GroupsUi;
use crate::ui::table::TableView;
use crate::ui::{advisor, charts, event_log as event_log_ui, table};
use crate::worker::{Command, Worker};

const INTERVALS_SECS: [u64; 5] = [1, 2, 5, 10, 30];
const DEFAULT_INTERVAL_SECS: u64 = 2;
/// Presets for removing APs that stopped answering, in seconds; 0 keeps them forever.
/// The default (one minute) is a few missed scans at the common intervals.
pub const STALE_PRESETS_SECS: [u64; 4] = [0, 60, 5 * 60, 15 * 60];
const CHART_WINDOWS_SECS: [f64; 4] = [60.0, 300.0, 900.0, 3600.0];

/// Delimiter choices for table export; semicolon suits spreadsheets in comma-decimal locales.
const EXPORT_DELIMITERS: [(&str, char); 3] = [("Comma separated…", ','), ("Semicolon separated…", ';'), ("Tab separated…", '\t')];

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartTab {
    Signal,
    Spectrum(Band),
    Advisor,
    Events,
}

mod settings;

pub struct Inspector {
    tracker: Tracker,
    adapters: Vec<Adapter>,
    adapter_index: usize,
    interval_secs: u64,
    pub paused: bool,
    filter: Filter,
    sort: Sort,
    selected: BTreeSet<MacAddress>,
    tab: ChartTab,
    window_index: usize,
    advisor_width_mhz: u16,
    alias_edit: Option<(MacAddress, String)>,
    alias_io: AliasIo,
    groups: CustomGroups,
    groups_ui: GroupsUi,
    log: EventLog,
    log_filter: EventFilter,
    stale_after_secs: u64,
    log_erase_secs: u64,
    sound_enabled: bool,
    /// Result of the last export/import action, shown in the status bar.
    notice: Option<String>,
    columns: ColumnSet,
    group_by: GroupBy,
    collapsed_groups: BTreeSet<String>,
    last_error: Option<String>,
    scans: u64,
}

/// What a new adapter list means for the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterChange {
    /// The adapter being scanned is still there.
    Unchanged,
    /// Scan on this adapter from now on.
    Switched(Adapter),
    /// No adapter remains.
    Lost,
}

impl Inspector {
    pub fn new(adapters: Vec<Adapter>) -> Self {
        Self {
            tracker: Tracker::default(),
            adapters,
            adapter_index: 0,
            interval_secs: DEFAULT_INTERVAL_SECS,
            paused: false,
            filter: Filter::default(),
            sort: Sort::default(),
            selected: BTreeSet::new(),
            tab: ChartTab::Signal,
            window_index: 1,
            advisor_width_mhz: 20,
            alias_edit: None,
            alias_io: AliasIo::default(),
            groups: CustomGroups::default(),
            groups_ui: GroupsUi::default(),
            log: EventLog::default(),
            log_filter: EventFilter::default(),
            stale_after_secs: 60,
            log_erase_secs: 60 * 60,
            sound_enabled: false,
            notice: None,
            columns: ColumnSet::default(),
            group_by: GroupBy::None,
            collapsed_groups: BTreeSet::new(),
            last_error: None,
            scans: 0,
        }
    }

    pub fn adapters(&self) -> &[Adapter] {
        &self.adapters
    }

    /// The adapter being scanned, `None` once the last one has been unplugged.
    pub fn active_adapter(&self) -> Option<&Adapter> {
        self.adapters.get(self.adapter_index)
    }

    /// Installs a new adapter list after a hot-plug event. The active adapter stays selected
    /// by identity; if it is gone the first remaining one takes over and the table starts
    /// over, as when the user picks another adapter.
    pub fn set_adapters(&mut self, list: Vec<Adapter>, now: f64) -> AdapterChange {
        let active_id = self.active_adapter().map(|a| a.id.clone());
        let choice = choose_active(&list, active_id.as_deref());
        self.adapters = list;
        match choice {
            ActiveAdapter::Keep(i) => {
                self.adapter_index = i;
                AdapterChange::Unchanged
            }
            ActiveAdapter::Switch(i) => {
                self.adapter_index = i;
                let adapter = self.adapters[i].clone();
                self.log.record(EventKind::AdapterChanged { name: adapter.description.clone() }, now, unix_now());
                self.log.reset_scan_baseline();
                self.tracker.clear();
                self.selected.clear();
                AdapterChange::Switched(adapter)
            }
            ActiveAdapter::None => {
                self.adapter_index = 0;
                AdapterChange::Lost
            }
        }
    }

    /// Custom AP groups, which Survey applies to its network list too.
    pub fn groups(&self) -> &CustomGroups {
        &self.groups
    }

    pub fn prefs(&self) -> InspectorPrefs {
        InspectorPrefs {
            interval_secs: self.interval_secs,
            window_index: self.window_index,
            advisor_width_mhz: self.advisor_width_mhz,
            ghz2_4: self.filter.ghz2_4,
            ghz5: self.filter.ghz5,
            ghz6: self.filter.ghz6,
            group_by: self.group_by.key().to_owned(),
            visible_columns: Some(self.columns.to_keys()),
            custom_groups: self.groups.to_saved(),
            stale_after_secs: self.stale_after_secs,
            log_erase_after_secs: self.log_erase_secs,
            sound_enabled: self.sound_enabled,
        }
    }

    /// Applies saved preferences, ignoring values that are no longer valid choices.
    pub fn apply_prefs(&mut self, p: &InspectorPrefs) {
        if INTERVALS_SECS.contains(&p.interval_secs) {
            self.interval_secs = p.interval_secs;
        }
        if p.window_index < CHART_WINDOWS_SECS.len() {
            self.window_index = p.window_index;
        }
        if [20, 40, 80, 160].contains(&p.advisor_width_mhz) {
            self.advisor_width_mhz = p.advisor_width_mhz;
        }
        self.filter.ghz2_4 = p.ghz2_4;
        self.filter.ghz5 = p.ghz5;
        self.filter.ghz6 = p.ghz6;
        self.group_by = GroupBy::from_key(&p.group_by).unwrap_or_default();
        self.columns = ColumnSet::from_keys(p.visible_columns.as_deref());
        self.groups = CustomGroups::from_saved(&p.custom_groups);
        self.sound_enabled = p.sound_enabled;
        if STALE_PRESETS_SECS.contains(&p.stale_after_secs) {
            self.stale_after_secs = p.stale_after_secs;
        }
        if ERASE_PRESETS_SECS.contains(&p.log_erase_after_secs) {
            self.log_erase_secs = p.log_erase_after_secs;
        }
    }

    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_secs)
    }

    /// Adds a scan. Returns the cue to play when sound is on and a network appeared that the
    /// previous scan did not have (one cue per scan, however many are new).
    pub fn ingest(&mut self, aps: Vec<AccessPoint>, now: f64) -> Option<Cue> {
        let diff = self.log.record_scan(&aps, now, unix_now());
        self.tracker.ingest(aps, now);
        self.last_error = None;
        self.scans += 1;
        (self.sound_enabled && diff.has_new_networks()).then_some(Cue::NewNetwork)
    }

    /// Shows a one-line message in the status bar.
    pub fn set_notice(&mut self, message: String) {
        self.notice = Some(message);
    }

    pub fn set_error(&mut self, message: String, now: f64) {
        self.log.record(EventKind::Error { message: message.clone(), repeats: 1 }, now, unix_now());
        self.last_error = Some(message);
    }

    pub fn housekeeping(&mut self, now: f64) {
        if self.stale_after_secs > 0 {
            self.tracker.prune(now, self.stale_after_secs as f64);
        }
        self.log.erase_older_than(now, self.log_erase_secs);
        self.selected.retain(|b| self.tracker.get(b).is_some());
    }

    fn controls(&mut self, ui: &mut egui::Ui, worker: &Worker, aliases: &Aliases, now: f64) {
        ui.horizontal_wrapped(|ui| {
            let current = self.active_adapter().map_or_else(|| "No adapter".to_owned(), |a| a.description.clone());
            egui::ComboBox::from_id_salt("adapter").selected_text(current).show_ui(ui, |ui| {
                for (i, a) in self.adapters.iter().enumerate() {
                    if ui.selectable_value(&mut self.adapter_index, i, &a.description).clicked() {
                        self.log.record(EventKind::AdapterChanged { name: a.description.clone() }, now, unix_now());
                        self.log.reset_scan_baseline();
                        self.tracker.clear();
                        self.selected.clear();
                        worker.send(Command::SetAdapter(a.clone()));
                    }
                }
            });

            let label = if self.paused { "▶ Resume" } else { "⏸ Pause" };
            if ui.button(label).clicked() {
                self.paused = !self.paused;
                let kind = if self.paused { EventKind::Paused } else { EventKind::Resumed };
                self.log.record(kind, now, unix_now());
                worker.send(Command::SetPaused(self.paused));
            }
            if ui.button("⟳ Scan now").clicked() {
                worker.send(Command::ScanNow);
            }

            egui::ComboBox::from_id_salt("interval")
                .selected_text(format!("every {} s", self.interval_secs))
                .show_ui(ui, |ui| {
                    for secs in INTERVALS_SECS {
                        if ui.selectable_label(self.interval_secs == secs, format!("every {secs} s")).clicked() {
                            self.set_interval(secs, worker, now);
                        }
                    }
                });

            ui.separator();
            ui.checkbox(&mut self.filter.ghz2_4, "2.4 GHz");
            ui.checkbox(&mut self.filter.ghz5, "5 GHz");
            ui.checkbox(&mut self.filter.ghz6, "6 GHz");
            ui.add(egui::TextEdit::singleline(&mut self.filter.text).hint_text("Filter SSID / BSSID / vendor").desired_width(180.0));
            ui.separator();
            self.group_and_columns_menu(ui, aliases, now);
            ui.menu_button("Aliases", |ui| self.alias_io.buttons(ui, aliases));
        });
    }

    /// Group-by picker and the column visibility menu.
    fn group_and_columns_menu(&mut self, ui: &mut egui::Ui, aliases: &Aliases, now: f64) {
        egui::ComboBox::from_id_salt("group_by")
            .selected_text(format!("Group: {}", self.group_by.label()))
            .show_ui(ui, |ui| {
                for g in GroupBy::ALL {
                    ui.selectable_value(&mut self.group_by, g, g.label());
                }
            });
        if ui.button("Groups…").on_hover_text("Create, rename and delete custom groups").clicked() {
            self.groups_ui.open_manager();
        }
        ui.menu_button("Export CSV", |ui| {
            for (label, delimiter) in EXPORT_DELIMITERS {
                if ui.button(label).clicked() {
                    self.export_table(aliases, now, delimiter);
                    ui.close();
                }
            }
        });
        ui.menu_button("Columns", |ui| {
            for c in Column::ALL {
                let mut shown = self.columns.contains(c);
                if ui.checkbox(&mut shown, c.title()).on_hover_text(c.description()).changed() {
                    // Hiding the last column is refused; the checkbox simply stays checked next frame.
                    self.columns.set(c, shown);
                }
            }
            ui.separator();
            if ui.button("Reset columns").clicked() {
                self.columns.reset();
            }
        });
    }

    /// Windows that can be opened from the preferences window while Survey is showing, so they
    /// must be drawn every frame regardless of mode.
    pub fn floating_windows(&mut self, ctx: &egui::Context, aliases: &mut Aliases) {
        self.alias_io.windows(ctx, aliases);
        if let Some(n) = self.alias_io.notice.take() {
            self.notice = Some(n);
        }
    }

    /// Writes the table as currently filtered and sorted.
    fn export_table(&mut self, aliases: &Aliases, now: f64, delimiter: char) {
        let rows = self.tracker.view(&self.filter, self.sort);
        let csv = table_export::to_csv(&rows, &self.columns, aliases, now, delimiter);
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("openspot-networks.csv").save_file() else {
            return;
        };
        self.notice = Some(match std::fs::write(&path, csv) {
            Ok(()) => format!("exported {} networks to {}", rows.len(), path.display()),
            Err(e) => format!("could not write {}: {e}", path.display()),
        });
    }

    pub fn show(&mut self, ctx: &egui::Context, worker: &Worker, now: f64, aliases: &mut Aliases) {
        egui::TopBottomPanel::top("controls").show(ctx, |ui| self.controls(ui, worker, aliases, now));

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} networks · {} scans", self.tracker.len(), self.scans));
                if self.paused {
                    ui.label("· paused");
                }
                if let Some(notice) = &self.notice {
                    ui.label(format!("· {notice}"));
                }
                if let Some(err) = &self.last_error {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("· {err}"));
                }
            });
        });

        let rows = self.tracker.view(&self.filter, self.sort);
        let all_aps = self.tracker.access_points();

        egui::TopBottomPanel::bottom("charts")
            .resizable(true)
            .default_height(320.0)
            .show(ctx, |ui| {
                chart_tabs(ui, &mut self.tab, &mut self.window_index);
                match self.tab {
                    ChartTab::Signal => charts::rssi_over_time(
                        ui,
                        &self.tracker,
                        &self.selected,
                        aliases,
                        now,
                        CHART_WINDOWS_SECS[self.window_index],
                    ),
                    ChartTab::Spectrum(band) => charts::spectrum(ui, &rows, band, &self.selected, aliases),
                    ChartTab::Advisor => advisor::show(ui, &all_aps, &mut self.advisor_width_mhz),
                    ChartTab::Events => {
                        let result = event_log_ui::show(ui, &mut self.log, &mut self.log_filter, &mut self.log_erase_secs);
                        self.notice = result.or(self.notice.take());
                    }
                }
            });
        crate::ui::alias::dialog(ctx, aliases, &mut self.alias_edit);
        self.groups_ui.windows(ctx, &mut self.groups);

        egui::CentralPanel::default().show(ctx, |ui| {
            if rows.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(if self.scans == 0 { "Scanning…" } else { "No networks match the filters." });
                });
            } else {
                let mut view = TableView {
                    sort: &mut self.sort,
                    selected: &mut self.selected,
                    aliases,
                    groups: &mut self.groups,
                    groups_ui: &mut self.groups_ui,
                    alias_request: &mut self.alias_edit,
                    columns: &self.columns,
                    group_by: self.group_by,
                    collapsed: &mut self.collapsed_groups,
                    now,
                };
                table::show(ui, &rows, &mut view);
            }
        });
    }
}

fn unix_now() -> i64 {
    // A clock before 1970 means a broken system clock; 0 is an obviously wrong but harmless stamp.
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn chart_tabs(ui: &mut egui::Ui, tab: &mut ChartTab, window_index: &mut usize) {
    ui.horizontal(|ui| {
        ui.selectable_value(tab, ChartTab::Signal, "Signal over time");
        ui.selectable_value(tab, ChartTab::Spectrum(Band::Ghz2_4), "2.4 GHz channels");
        ui.selectable_value(tab, ChartTab::Spectrum(Band::Ghz5), "5 GHz channels");
        ui.selectable_value(tab, ChartTab::Spectrum(Band::Ghz6), "6 GHz channels");
        ui.selectable_value(tab, ChartTab::Advisor, "Channel advisor");
        ui.selectable_value(tab, ChartTab::Events, "Event log");
        if *tab == ChartTab::Signal {
            ui.separator();
            egui::ComboBox::from_id_salt("window")
                .selected_text(window_label(CHART_WINDOWS_SECS[*window_index]))
                .show_ui(ui, |ui| {
                    for (i, w) in CHART_WINDOWS_SECS.iter().enumerate() {
                        ui.selectable_value(window_index, i, window_label(*w));
                    }
                });
        }
    });
}

fn window_label(secs: f64) -> String {
    if secs < 120.0 { format!("last {secs:.0} s") } else { format!("last {:.0} min", secs / 60.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_and_columns_survive_a_prefs_round_trip() {
        let mut a = Inspector::new(vec![]);
        a.group_by = GroupBy::Vendor;
        a.columns.set(Column::Type, true);
        a.columns.set(Column::Vendor, false);
        let json = serde_json::to_string(&a.prefs()).unwrap();
        let mut b = Inspector::new(vec![]);
        b.apply_prefs(&serde_json::from_str(&json).unwrap());
        assert_eq!(b.group_by, GroupBy::Vendor);
        assert_eq!(b.columns, a.columns);
        assert!(b.columns.contains(Column::Type) && !b.columns.contains(Column::Vendor));
    }

    #[test]
    fn unknown_group_key_falls_back_to_no_grouping() {
        let mut a = Inspector::new(vec![]);
        a.apply_prefs(&InspectorPrefs { group_by: "from_the_future".into(), ..InspectorPrefs::default() });
        assert_eq!(a.group_by, GroupBy::None);
    }

    #[test]
    fn custom_groups_persist_via_prefs_and_survive_old_files() {
        let mut a = Inspector::new(vec![]);
        a.groups.create("Lobby").unwrap();
        a.groups.assign(&MacAddress([0xAA, 0, 0, 0, 0, 1]), Some("Lobby")).unwrap();
        let json = serde_json::to_string(&a.prefs()).unwrap();
        let mut b = Inspector::new(vec![]);
        b.apply_prefs(&serde_json::from_str(&json).unwrap());
        assert_eq!(b.groups, a.groups);
        let old: InspectorPrefs = serde_json::from_str(r#"{"interval_secs":2}"#).unwrap();
        assert!(old.custom_groups.is_empty());
    }

    fn beacon(last: u8) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), -50, 2_437_000, true, 100, vec![])
    }

    #[test]
    fn ingest_logs_scans_and_reports_new_networks() {
        let mut i = Inspector::new(vec![]);
        i.sound_enabled = true;
        assert_eq!(i.ingest(vec![beacon(1)], 0.0), None, "the first scan is the initial inventory");
        assert_eq!(i.ingest(vec![beacon(1)], 2.0), None, "nothing new");
        assert_eq!(i.ingest(vec![beacon(1), beacon(2)], 4.0), Some(Cue::NewNetwork));
        i.sound_enabled = false;
        assert_eq!(i.ingest(vec![beacon(1), beacon(2), beacon(3)], 6.0), None, "sound is opt-in");
        i.set_error("boom".into(), 3.0);
        i.set_error("boom".into(), 4.0);
        assert_eq!(i.log.len(), 5, "four scans and one collapsed error");
    }

    #[test]
    fn stale_setting_controls_pruning_and_zero_means_never() {
        let mut i = Inspector::new(vec![]);
        i.ingest(vec![beacon(1)], 0.0);
        i.stale_after_secs = 0;
        i.housekeeping(10_000.0);
        assert_eq!(i.tracker.len(), 1, "never");
        i.stale_after_secs = 60;
        i.housekeeping(10_000.0);
        assert_eq!(i.tracker.len(), 0);
    }

    #[test]
    fn log_erase_runs_with_housekeeping_and_prefs_reject_unknown_presets() {
        let mut i = Inspector::new(vec![]);
        i.ingest(vec![beacon(1)], 0.0);
        i.housekeeping(10.0 * 3600.0);
        assert!(i.log.is_empty(), "default one-hour erase removed the old scan event");
        i.apply_prefs(&InspectorPrefs { stale_after_secs: 7, log_erase_after_secs: 15 * 60, ..InspectorPrefs::default() });
        assert_eq!(i.stale_after_secs, 60, "7 s is not an offered preset, so the default stays");
        assert_eq!(i.log_erase_secs, 15 * 60);
        assert_eq!(i.prefs().log_erase_after_secs, 15 * 60);
    }

    fn adapter(n: u8) -> Adapter {
        Adapter { id: format!("id-{n}"), description: format!("Adapter {n}") }
    }

    #[test]
    fn replugging_keeps_the_active_adapter_and_its_networks() {
        let mut i = Inspector::new(vec![adapter(1), adapter(2)]);
        i.adapter_index = 1;
        i.ingest(vec![beacon(1)], 0.0);
        assert_eq!(i.set_adapters(vec![adapter(3), adapter(2)], 1.0), AdapterChange::Unchanged);
        assert_eq!(i.active_adapter(), Some(&adapter(2)), "selected by identity, not position");
        assert_eq!(i.tracker.len(), 1);
    }

    #[test]
    fn losing_the_active_adapter_switches_to_another_and_starts_the_table_over() {
        let mut i = Inspector::new(vec![adapter(1), adapter(2)]);
        i.ingest(vec![beacon(1)], 0.0);
        assert_eq!(i.set_adapters(vec![adapter(2)], 1.0), AdapterChange::Switched(adapter(2)));
        assert_eq!(i.active_adapter(), Some(&adapter(2)));
        assert_eq!(i.tracker.len(), 0, "networks of the old adapter are not carried over");
    }

    #[test]
    fn unplugging_the_last_adapter_and_plugging_one_back_recovers() {
        let mut i = Inspector::new(vec![adapter(1)]);
        assert_eq!(i.set_adapters(vec![], 1.0), AdapterChange::Lost);
        assert_eq!(i.active_adapter(), None);
        assert_eq!(i.set_adapters(vec![adapter(5)], 2.0), AdapterChange::Switched(adapter(5)));
        assert_eq!(i.active_adapter(), Some(&adapter(5)));
    }
}
