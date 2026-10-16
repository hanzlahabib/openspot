//! Survey UI: offer to recover a newer autosave after opening a project.

use eframe::egui::{self, Align2, Vec2};

use crate::survey::Survey;

use super::toolbar::report;

pub(super) fn dialog(s: &mut Survey, ctx: &egui::Context) {
    if s.autosave.recovery.is_none() {
        return;
    }
    let (mut recover, mut discard) = (false, false);
    egui::Window::new("Recover unsaved work").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.label("An autosave newer than this file was found. The app may have closed before saving.");
        ui.horizontal(|ui| {
            recover = ui.button("Recover").clicked();
            discard = ui.button("Discard autosave").clicked();
        });
    });
    if recover {
        let r = s.recover_autosave();
        report(s, r, "Recovered from autosave. Save to keep it.");
    } else if discard {
        s.discard_autosave();
    }
}
