//! Getting a zone ready for planning: a floor plan image and a scale.
//! UNVERIFIED: egui layer written without a display.

use eframe::egui;

use crate::planning::Planning;
use crate::survey::Survey;

/// Shown instead of the canvas while the zone has no floor plan.
pub(super) fn no_plan(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        ui.heading("Start a plan");
        ui.label("Planning draws on the same floor plan as Survey. Load an image, or start from an empty grid of a known size.");
        ui.add_space(8.0);
        if ui.button("Open floor plan image…").clicked()
            && let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file()
        {
            match s.load_map(&path) {
                Ok(()) => s.dirty = true,
                Err(e) => p.notice = Some(e),
            }
        }
        ui.label("PDF plans: open them in Survey mode first.");
        ui.add_space(8.0);
        let (mut w, mut h) = p.blank_dialog.unwrap_or((30.0, 20.0));
        ui.horizontal(|ui| {
            ui.label("Blank plan:");
            ui.add(egui::DragValue::new(&mut w).range(1.0..=1000.0).suffix(" m"));
            ui.label("×");
            ui.add(egui::DragValue::new(&mut h).range(1.0..=1000.0).suffix(" m"));
            if ui.button("Create").clicked() {
                match s.create_blank_map(w, h) {
                    Ok(()) => s.dirty = true,
                    Err(e) => p.notice = Some(e),
                }
            }
        });
        p.blank_dialog = Some((w, h));
    });
}

/// Banner while the zone has a plan but no scale: drawing needs real distances.
pub(super) fn scale_banner(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.colored_label(egui::Color32::YELLOW, "This plan has no scale, so nothing can be drawn or predicted yet.");
        ui.label("Plan width:");
        ui.add(egui::DragValue::new(&mut p.assumed_width_m).range(1.0..=2000.0).suffix(" m"));
        if ui.button("Set scale").on_hover_text("Treats the image's full width as this many meters. For a measured scale use Calibrate in Survey mode.").clicked() {
            let width = p.assumed_width_m;
            let zone = s.zone;
            match Planning::set_scale_from_width(&mut s.project.zones[zone], width) {
                Ok(_) => s.dirty = true,
                Err(e) => p.notice = Some(e),
            }
        }
    });
}
