//! The Preferences window: a category list on the left, the selected page on the right.

use eframe::egui::{self, Align2, Vec2};
use openspot_heatmap::{GradientPreset, Smoothing};
use openspot_model::LengthUnit;

use openspot_i18n::{Args, available_locales, preference, set_preference, tr, tr_args};

use super::dialog::escape_pressed;
use crate::aliases::{AliasScope, Aliases};
use crate::inspector::Inspector;
use crate::sound::SoundNotifier;
use crate::survey::{MIN_OPACITY, Survey};
use crate::update_check::{CURRENT_VERSION, UpdateChecker, UpdateStatus};
use crate::prefs::UpdatePrefs;
use crate::theme::{Appearance, SCALE_PRESETS_PERCENT, ThemeChoice};
use crate::worker::Worker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Inspector,
    Survey,
    Heatmap,
    Aliases,
    Appearance,
    Backup,
    Privacy,
    Updates,
}

impl Category {
    const ALL: [Self; 8] =
        [Self::Inspector, Self::Survey, Self::Heatmap, Self::Aliases, Self::Appearance, Self::Backup, Self::Privacy, Self::Updates];

    fn label(self) -> String {
        match self {
            Self::Inspector => tr("prefs-cat-inspector"),
            Self::Survey => tr("prefs-cat-survey"),
            Self::Heatmap => tr("prefs-cat-heatmap"),
            Self::Aliases => tr("prefs-cat-aliases"),
            Self::Appearance => tr("prefs-cat-appearance"),
            Self::Backup => tr("prefs-cat-backup"),
            Self::Privacy => tr("prefs-cat-privacy"),
            Self::Updates => tr("prefs-cat-updates"),
        }
    }
}

#[derive(Debug)]
pub struct PrefsWindow {
    pub open: bool,
    category: Category,
}

impl Default for PrefsWindow {
    fn default() -> Self {
        Self { open: false, category: Category::Inspector }
    }
}

/// What the window edits. Bundled because the pages touch several unrelated owners.
pub struct PrefsContext<'a> {
    pub inspector: &'a mut Inspector,
    pub survey: &'a mut Survey,
    pub aliases: &'a Aliases,
    pub alias_scope: &'a mut AliasScope,
    pub appearance: &'a mut Appearance,
    pub sound: &'a mut SoundNotifier,
    pub updates: &'a mut UpdatePrefs,
    pub checker: &'a mut UpdateChecker,
    pub worker: &'a Worker,
    pub now: f64,
}

impl PrefsWindow {
    pub fn show(&mut self, ctx: &egui::Context, pc: &mut PrefsContext<'_>) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("prefs-title"))
            // Fixed id: the title changes with the language and would otherwise reset the window.
            .id(egui::Id::new("preferences_window"))
            .open(&mut open)
            .collapsible(false)
            .default_size([560.0, 360.0])
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_min_width(110.0);
                        for c in Category::ALL {
                            ui.selectable_value(&mut self.category, c, c.label());
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.heading(self.category.label());
                        ui.add_space(6.0);
                        page(ui, self.category, pc);
                    });
                });
            });
        if escape_pressed(ctx) {
            open = false;
        }
        self.open = open;
    }
}

fn page(ui: &mut egui::Ui, category: Category, pc: &mut PrefsContext<'_>) {
    match category {
        Category::Inspector => pc.inspector.settings_page(ui, pc.worker, pc.now, pc.sound),
        Category::Survey => survey_page(ui, pc.survey),
        Category::Heatmap => heatmap_page(ui, pc.survey),
        Category::Aliases => pc.inspector.aliases_page(ui, pc.aliases, pc.alias_scope),
        Category::Appearance => appearance_page(ui, pc.appearance),
        Category::Backup => {
            ui.label(tr("prefs-backup-unavailable"));
            ui.weak(tr("prefs-backup-bak"));
        }
        Category::Privacy => {
            ui.label(tr("prefs-privacy-no-telemetry"));
            ui.label(tr("prefs-privacy-local"));
            ui.label(tr("prefs-privacy-network"));
        }
        Category::Updates => updates_page(ui, pc),
    }
}

/// Runs `edit` on a copy of the survey preferences and applies it only if something changed.
fn edit_survey(survey: &mut Survey, edit: impl FnOnce(&mut crate::prefs::SurveyPrefs)) {
    let before = survey.prefs();
    let mut after = before.clone();
    edit(&mut after);
    if after != before {
        survey.apply_prefs(&after);
    }
}

fn survey_page(ui: &mut egui::Ui, survey: &mut Survey) {
    edit_survey(survey, |p| {
        ui.horizontal(|ui| {
            ui.label(tr("prefs-length-unit"));
            egui::ComboBox::from_id_salt("pref_unit")
                .selected_text(LengthUnit::ALL.get(p.unit).map_or("m", |u| u.label()))
                .show_ui(ui, |ui| {
                    for (i, u) in LengthUnit::ALL.iter().enumerate() {
                        ui.selectable_value(&mut p.unit, i, u.label());
                    }
                });
        });
        ui.checkbox(&mut p.show_heatmap, tr("prefs-show-heatmap"));
        ui.checkbox(&mut p.show_points, tr("prefs-show-points"));
        ui.checkbox(&mut p.show_ap_markers, tr("prefs-show-ap-markers"));
        ui.checkbox(&mut p.sound_enabled, tr("prefs-survey-sound"));
    });
}

fn heatmap_page(ui: &mut egui::Ui, survey: &mut Survey) {
    edit_survey(survey, |p| {
        ui.horizontal(|ui| {
            ui.label(tr("prefs-gradient"));
            egui::ComboBox::from_id_salt("pref_gradient")
                .selected_text(GradientPreset::ALL.get(p.gradient).map_or("", |g| g.label()))
                .show_ui(ui, |ui| {
                    for (i, g) in GradientPreset::ALL.iter().enumerate() {
                        ui.selectable_value(&mut p.gradient, i, g.label());
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(tr("prefs-smoothing"));
            egui::ComboBox::from_id_salt("pref_smoothing")
                .selected_text(Smoothing::ALL.get(p.smoothing).map_or("", |s| s.label()))
                .show_ui(ui, |ui| {
                    for (i, s) in Smoothing::ALL.iter().enumerate() {
                        ui.selectable_value(&mut p.smoothing, i, s.label());
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(tr("prefs-opacity"));
            ui.add(egui::Slider::new(&mut p.opacity, MIN_OPACITY..=u8::MAX));
        });
    });
}

fn appearance_page(ui: &mut egui::Ui, a: &mut Appearance) {
    ui.label(tr("prefs-theme"));
    for t in ThemeChoice::ALL {
        ui.radio_value(&mut a.theme, t, t.label());
    }
    ui.add_space(6.0);
    ui.checkbox(&mut a.high_contrast, tr("prefs-high-contrast"))
        .on_hover_text(tr("prefs-high-contrast-hint"));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(tr("prefs-interface-size"));
        egui::ComboBox::from_id_salt("pref_scale").selected_text(scale_label(a.scale_percent)).show_ui(ui, |ui| {
            for p in SCALE_PRESETS_PERCENT {
                ui.selectable_value(&mut a.scale_percent, p, scale_label(p));
            }
        });
    });
    ui.add_space(6.0);
    ui.add_space(6.0);
    language_selector(ui);
    ui.weak(tr("prefs-keyboard-hint"));
}

fn scale_label(percent: u16) -> String {
    let mut args = Args::new();
    args.set("percent", percent);
    tr_args("prefs-scale-option", &args)
}

/// Applies the choice immediately; the app persists it with the other preferences.
fn language_selector(ui: &mut egui::Ui) {
    let current = preference();
    let locales = available_locales();
    let shown = match &current {
        None => tr("prefs-language-system"),
        Some(tag) => locales.iter().find(|l| &l.tag == tag).map_or_else(|| tag.clone(), |l| l.name.clone()),
    };
    ui.horizontal(|ui| {
        ui.label(tr("prefs-language"));
        egui::ComboBox::from_id_salt("pref_language").selected_text(shown).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), tr("prefs-language-system")).clicked() {
                set_preference(None);
            }
            for l in &locales {
                if ui.selectable_label(current.as_deref() == Some(l.tag.as_str()), &l.name).clicked() {
                    set_preference(Some(&l.tag));
                }
            }
        });
    });
    ui.weak(tr("prefs-language-hint"));
    ui.add_space(6.0);
}

fn updates_page(ui: &mut egui::Ui, pc: &mut PrefsContext<'_>) {
    let mut version = Args::new();
    version.set("version", CURRENT_VERSION);
    ui.label(tr_args("prefs-updates-version", &version));
    ui.checkbox(&mut pc.updates.check_on_startup, tr("prefs-updates-check-on-start"));
    ui.weak(tr("prefs-updates-hint"));
    ui.add_space(6.0);
    if ui.add_enabled(!pc.checker.is_checking(), egui::Button::new(tr("prefs-updates-check-now"))).clicked() {
        pc.checker.start(ui.ctx());
    }
    if pc.checker.is_checking() {
        ui.label(tr("prefs-updates-checking"));
    }
    match &pc.checker.result {
        Some(Ok(UpdateStatus::UpToDate)) => {
            ui.label(tr("prefs-updates-latest"));
        }
        Some(Ok(UpdateStatus::Available { version, url })) => {
            let mut args = Args::new();
            args.set("version", version.as_str());
            ui.label(tr_args("prefs-updates-available", &args));
            ui.hyperlink(url);
        }
        Some(Err(e)) => {
            ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
        }
        None => {}
    }
}
