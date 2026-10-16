//! Survey UI: per-AP coverage quick switch.

use eframe::egui;

use crate::aliases::Aliases;
use crate::survey::Survey;

pub(super) fn switcher(s: &mut Survey, ui: &mut egui::Ui, aliases: &Aliases) {
    let label = match s.per_ap {
        Some(b) => {
            let ssid = s.snapshot().networks().into_iter().find(|n| n.0 == b).map(|n| n.1).unwrap_or_default();
            aliases.name_for(&b, &ssid)
        }
        None => "All networks".to_owned(),
    };
    ui.horizontal(|ui| {
        ui.label("Map:");
        if ui.small_button("◀").on_hover_text("Previous network's coverage").clicked() {
            s.step_per_ap(-1);
        }
        ui.label(label);
        if ui.small_button("▶").on_hover_text("Next network's coverage").clicked() {
            s.step_per_ap(1);
        }
        if ui.add_enabled(s.per_ap.is_some(), egui::Button::new("All").small()).clicked() {
            s.set_per_ap(None);
        }
    });
}
