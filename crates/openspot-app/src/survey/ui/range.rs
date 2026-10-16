//! Survey UI: value-range inputs under the gradient legend.

use eframe::egui;

use crate::survey::Survey;

pub(super) fn inputs(s: &mut Survey, ui: &mut egui::Ui) {
    let (mut lo, mut hi) = s.effective_range();
    let unit = s.base_metric_unit();
    let changed = ui
        .horizontal(|ui| {
            ui.label("Range");
            let a = ui.add(egui::DragValue::new(&mut lo).suffix(format!(" {unit}")));
            ui.label("to");
            let b = ui.add(egui::DragValue::new(&mut hi).suffix(format!(" {unit}")));
            a.changed() || b.changed()
        })
        .inner;
    if changed {
        // Rejected (too narrow) edits snap back to the previous range on the next frame.
        s.set_range((lo, hi));
    }
    if ui.add_enabled(s.active_range().is_some(), egui::Button::new("Reset range").small()).clicked() {
        s.reset_range();
    }
}
