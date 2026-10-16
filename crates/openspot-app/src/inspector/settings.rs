//! Inspector pages of the preferences window.

use std::time::Duration;

use eframe::egui;

use super::{CHART_WINDOWS_SECS, INTERVALS_SECS, Inspector, STALE_PRESETS_SECS, unix_now, window_label};
use crate::aliases::{AliasScope, Aliases};
use crate::event_log::{ERASE_PRESETS_SECS, EventKind};
use crate::sound::{Cue, SoundNotifier};
use crate::ui::presets::duration_combo;
use crate::worker::{Command, Worker};

impl Inspector {
    /// Changes the scan interval, telling the worker and logging it.
    pub(super) fn set_interval(&mut self, secs: u64, worker: &Worker, now: f64) {
        self.interval_secs = secs;
        self.log.record(EventKind::IntervalChanged { secs }, now, unix_now());
        worker.send(Command::SetInterval(Duration::from_secs(secs)));
    }

    /// "Inspector" page: scanning, chart and retention settings.
    pub fn settings_page(&mut self, ui: &mut egui::Ui, worker: &Worker, now: f64, sound: &mut SoundNotifier) {
        egui::Grid::new("inspector_settings").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Scan interval");
            let mut secs = self.interval_secs;
            egui::ComboBox::from_id_salt("pref_interval").selected_text(format!("every {secs} s")).show_ui(ui, |ui| {
                for s in INTERVALS_SECS {
                    ui.selectable_value(&mut secs, s, format!("every {s} s"));
                }
            });
            if secs != self.interval_secs {
                self.set_interval(secs, worker, now);
            }
            ui.end_row();

            ui.label("Signal chart window");
            egui::ComboBox::from_id_salt("pref_window").selected_text(window_label(CHART_WINDOWS_SECS[self.window_index])).show_ui(ui, |ui| {
                for (i, w) in CHART_WINDOWS_SECS.iter().enumerate() {
                    ui.selectable_value(&mut self.window_index, i, window_label(*w));
                }
            });
            ui.end_row();

            ui.label("Remove networks not heard for");
            duration_combo(ui, "pref_stale", &mut self.stale_after_secs, &STALE_PRESETS_SECS);
            ui.end_row();

            ui.label("Erase event log entries older than");
            duration_combo(ui, "pref_log_erase", &mut self.log_erase_secs, &ERASE_PRESETS_SECS);
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.sound_enabled, "Play a sound when a new network appears").changed() {
                sound.reset();
            }
            if ui.button("Test sound").clicked() {
                sound.reset();
                self.notice = Some(sound.play(Cue::NewNetwork).unwrap_or_else(|| "played the test sound".to_owned()));
            }
        });
        ui.add_space(8.0);
        ui.weak("Pausing a scan is not a setting: it stays as you left it. \"Never\" keeps networks and log entries until you quit (the log is also capped in size).");
    }

    /// "Aliases" page: where they are kept, import, export and forget.
    pub fn aliases_page(&mut self, ui: &mut egui::Ui, aliases: &Aliases, scope: &mut AliasScope) {
        ui.label("Keep aliases");
        ui.radio_value(scope, AliasScope::App, "App-wide: one list for every project, shared by Inspector and Survey");
        ui.radio_value(scope, AliasScope::Project, "Per project: stored in the project file and shared with whoever opens it");
        ui.add_space(8.0);
        ui.label(format!("{} {} aliases are shown and edited now.", aliases.len(), aliases.scope().label()));
        ui.weak("Switching only changes which list is used; the other list is kept and comes back when you switch again.");
        ui.add_space(8.0);
        self.alias_io.buttons(ui, aliases);
    }
}
