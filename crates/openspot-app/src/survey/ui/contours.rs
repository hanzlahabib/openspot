//! Survey UI: contour lines over the heatmap.

use eframe::egui::{self, Color32, Pos2, Stroke};

use crate::survey::Survey;

/// Contour line width on screen; constant regardless of zoom so lines stay readable.
const LINE_WIDTH_PX: f32 = 1.8;

pub(super) fn draw(s: &mut Survey, painter: &egui::Painter, origin: Pos2) {
    if !s.show_contours {
        return;
    }
    let gradient = openspot_heatmap::effective_gradient(&s.metric(), s.gradient, s.active_range());
    let view = s.view;
    for iso in s.contour_lines() {
        let [r, g, b] = gradient.color(iso.level);
        // Outline in black under the colored line so it reads on any plan.
        for line in &iso.lines {
            let pts: Vec<Pos2> = line.points.iter().map(|p| view.to_screen(origin, *p)).collect();
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(LINE_WIDTH_PX + 1.5, Color32::from_black_alpha(160))));
            painter.add(egui::Shape::line(pts, Stroke::new(LINE_WIDTH_PX, Color32::from_rgb(r, g, b))));
        }
    }
}
