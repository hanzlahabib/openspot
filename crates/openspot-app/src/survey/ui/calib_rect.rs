//! Survey UI: rectangle calibration dialog.

use eframe::egui::{self, Align2, Color32, Vec2};
use openspot_model::ASPECT_WARN_FRACTION;

use crate::survey::{Calibration, Survey};
use crate::survey::calibration::rect_mismatch;

pub(super) fn dialog(s: &mut Survey, ctx: &egui::Context) {
    let Calibration::RectEnter { a, b, .. } = s.calibration else { return };
    let (mut apply, mut cancel) = (false, false);
    egui::Window::new("Set scale from rectangle").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.label(format!("The rectangle is {:.0} × {:.0} px. How big is it in real life?", (a.0 - b.0).abs(), (a.1 - b.1).abs()));
        if let Calibration::RectEnter { width_m, height_m, .. } = &mut s.calibration {
            let unit = s.unit;
            let range = unit.from_meters(0.05)..=unit.from_meters(10_000.0);
            let (mut w, mut h) = (unit.from_meters(*width_m), unit.from_meters(*height_m));
            ui.horizontal(|ui| {
                ui.label("Width");
                ui.add(egui::DragValue::new(&mut w).speed(0.1).range(range.clone()).suffix(format!(" {}", unit.label())));
                ui.label("Height");
                ui.add(egui::DragValue::new(&mut h).speed(0.1).range(range).suffix(format!(" {}", unit.label())));
            });
            (*width_m, *height_m) = (unit.to_meters(w), unit.to_meters(h));
        }
        if let Some(mismatch) = rect_mismatch(&s.calibration).filter(|m| *m > ASPECT_WARN_FRACTION) {
            ui.colored_label(
                Color32::YELLOW,
                format!("The clicked shape's proportions differ from these sizes by {:.0} %. Check the corners and the width/height order; the scale will be an average.", mismatch * 100.0),
            );
        }
        ui.horizontal(|ui| {
            apply = ui.button("Apply").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if apply {
        if let Some(fit) = s.calibration.confirm_rect() {
            s.set_scale(fit.scale);
            s.message = Some(if fit.exceeds(ASPECT_WARN_FRACTION) {
                format!("Scale set, but the proportions were {:.0} % off; re-check the corners", fit.aspect_mismatch * 100.0)
            } else {
                "Scale set".to_owned()
            });
        }
    } else if cancel {
        s.calibration.cancel();
    }
}
