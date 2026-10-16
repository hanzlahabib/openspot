//! Survey UI: crop controls and outline.

use eframe::egui::{self, Color32, Pos2, Stroke};
use openspot_model::CropArea;

use crate::survey::Survey;
use crate::survey::crop::CropMode;

const CROP_COLOR: Color32 = Color32::from_rgb(255, 140, 0);
const CROP_WIDTH: f32 = 2.0;
const VERTEX_RADIUS: f32 = 4.0;

pub(super) fn bar(s: &mut Survey, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        let mut mode = s.crop_draft.mode;
        ui.selectable_value(&mut mode, CropMode::Rect, "Rectangle");
        ui.selectable_value(&mut mode, CropMode::Polygon, "Polygon");
        s.crop_set_mode(mode);
        if mode == CropMode::Polygon {
            let ready = s.crop_draft.points.len() >= 3;
            if ui.add_enabled(ready, egui::Button::new("Finish polygon")).clicked() {
                s.crop_finish_polygon();
            }
            if ui.add_enabled(!s.crop_draft.points.is_empty(), egui::Button::new("Remove last")).clicked() {
                s.crop_draft.remove_last();
            }
        }
        if ui.add_enabled(s.crop().is_some(), egui::Button::new("Remove crop")).clicked() {
            s.set_crop(None);
        }
        let mut gray = s.gray_unmeasured();
        if ui.checkbox(&mut gray, "Gray out unmeasured areas").on_hover_text("Tint everything farther than the data point radius from any measurement").changed() {
            s.set_gray_unmeasured(gray);
        }
    });
}

/// Outline of the active crop and of the one being drawn.
pub(super) fn draw(s: &Survey, painter: &egui::Painter, origin: Pos2) {
    let at = |p: (f64, f64)| s.view.to_screen(origin, p);
    let stroke = Stroke::new(CROP_WIDTH, CROP_COLOR);
    match s.crop() {
        Some(CropArea::Rect { min, max }) => {
            painter.rect_stroke(egui::Rect::from_two_pos(at(*min), at(*max)), 0.0, stroke, egui::StrokeKind::Middle);
        }
        Some(CropArea::Polygon(pts)) => {
            let mut line: Vec<Pos2> = pts.iter().map(|&p| at(p)).collect();
            line.push(line[0]);
            painter.add(egui::Shape::line(line, stroke));
        }
        None => {}
    }
    let draft: Vec<Pos2> = s.crop_draft.points.iter().map(|&p| at(p)).collect();
    if draft.len() > 1 {
        painter.add(egui::Shape::line(draft.clone(), stroke));
    }
    for p in draft {
        painter.circle_filled(p, VERTEX_RADIUS, CROP_COLOR);
    }
}
