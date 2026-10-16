//! Small helpers shared by modal-style windows.

use eframe::egui;

/// Whether Escape was pressed this frame; modal-style windows treat it as Cancel.
pub fn escape_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
}
