//! Event log viewer: filter, retention setting and export.

use std::fs;

use eframe::egui::{self, RichText};

use super::presets::duration_combo;
use crate::event_log::{Category, ERASE_PRESETS_SECS, EventFilter, EventKind, EventLog, format_utc};

const ROW_HEIGHT: f32 = 18.0;

/// Which export format the user chose.
#[derive(Clone, Copy)]
enum Export {
    Text,
    Csv,
}

/// Draws the log, newest first. Returns a message when an export succeeded or failed.
pub fn show(ui: &mut egui::Ui, log: &mut EventLog, filter: &mut EventFilter, erase_secs: &mut u64) -> Option<String> {
    let mut notice = None;
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::TextEdit::singleline(&mut filter.text).hint_text("Filter events").desired_width(160.0));
        for c in Category::ALL {
            let mut shown = filter.shows(c);
            if ui.checkbox(&mut shown, c.label()).changed() {
                filter.set_shown(c, shown);
            }
        }
        ui.separator();
        ui.label("Erase older than");
        duration_combo(ui, "log_erase", erase_secs, &ERASE_PRESETS_SECS);
        ui.separator();
        if ui.button("Export text…").clicked() {
            notice = export(log, filter, Export::Text);
        }
        if ui.button("Export CSV…").clicked() {
            notice = export(log, filter, Export::Csv);
        }
        if ui.button("Clear").clicked() {
            log.clear();
        }
    });
    ui.separator();

    let events = log.filtered(filter);
    if events.is_empty() {
        ui.weak(if log.is_empty() { "Nothing logged yet." } else { "No events match the filter." });
        return notice;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_HEIGHT, events.len(), |ui, range| {
        for e in events.iter().rev().skip(range.start).take(range.len()) {
            ui.horizontal(|ui| {
                ui.monospace(format_utc(e.unix_secs));
                let text = RichText::new(e.kind.message());
                if matches!(e.kind, EventKind::Error { .. }) {
                    ui.label(text.color(egui::Color32::LIGHT_RED));
                } else {
                    ui.label(text);
                }
            });
        }
    });
    notice
}

fn export(log: &EventLog, filter: &EventFilter, format: Export) -> Option<String> {
    let (name, ext, body) = match format {
        Export::Text => ("openspot-events.txt", "txt", log.to_text(filter)),
        Export::Csv => ("openspot-events.csv", "csv", log.to_csv(filter)),
    };
    let path = rfd::FileDialog::new().add_filter(ext, &[ext]).set_file_name(name).save_file()?;
    Some(match fs::write(&path, body) {
        Ok(()) => format!("exported event log to {}", path.display()),
        Err(e) => format!("could not write {}: {e}", path.display()),
    })
}
