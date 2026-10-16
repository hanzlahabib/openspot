//! Survey UI: PDF page chooser.

use eframe::egui::{self, Align2, Vec2};

use crate::survey::Survey;

pub(super) fn dialog(s: &mut Survey, ctx: &egui::Context) {
    let Some(mut choice) = s.pdf_choice.clone() else { return };
    let (mut load, mut cancel) = (false, false);
    egui::Window::new("Choose a PDF page").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.label(format!("“{}” has {} pages. Which one is the floor plan?", choice.name, choice.page_count));
        ui.horizontal(|ui| {
            ui.label("Page");
            ui.add(egui::DragValue::new(&mut choice.page).range(1..=choice.page_count));
            ui.label(format!("of {}", choice.page_count));
        });
        ui.horizontal(|ui| {
            load = ui.button("Load").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if load {
        s.pdf_choice = Some(choice);
        let r = s.confirm_pdf_choice();
        super::toolbar::report(s, r, "Floor plan loaded. Calibrate the scale next.");
    } else if cancel {
        s.pdf_choice = None;
    } else {
        s.pdf_choice = Some(choice);
    }
}
