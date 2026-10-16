//! Heatmap display pieces shared by Survey and Planning: style controls, the gradient
//! legend and hover value text.

use eframe::egui::{self, Color32, Rect, Sense, pos2, vec2};
use openspot_heatmap::{Gradient, GradientPreset, Grid, Metric, Smoothing};

use crate::survey::MIN_OPACITY;

/// Slices of the legend color bar; enough to look continuous at legend width.
const LEGEND_SLICES: usize = 48;
/// Widest the legend bar grows.
const LEGEND_MAX_WIDTH: f32 = 240.0;

pub fn gradient_combo(ui: &mut egui::Ui, id: &str, preset: &mut GradientPreset) {
    egui::ComboBox::from_id_salt(id).selected_text(preset.label()).show_ui(ui, |ui| {
        for p in GradientPreset::ALL {
            ui.selectable_value(preset, p, p.label());
        }
    });
}

pub fn smoothing_combo(ui: &mut egui::Ui, id: &str, smoothing: &mut Smoothing) {
    egui::ComboBox::from_id_salt(id).selected_text(format!("Smoothing: {}", smoothing.label())).show_ui(ui, |ui| {
        for level in Smoothing::ALL {
            ui.selectable_value(smoothing, level, level.label());
        }
    });
}

pub fn opacity_slider(ui: &mut egui::Ui, opacity: &mut u8) {
    ui.add(egui::Slider::new(opacity, MIN_OPACITY..=u8::MAX).text("opacity"));
}

/// Color bar over `gradient`'s value range with the end values and `unit` underneath.
pub fn gradient_bar(ui: &mut egui::Ui, gradient: &Gradient, unit: &str) {
    let (lo, hi) = gradient.range();
    let width = ui.available_width().min(LEGEND_MAX_WIDTH);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), Sense::hover());
    for i in 0..LEGEND_SLICES {
        let t = (i as f32 + 0.5) / LEGEND_SLICES as f32;
        let [r, g, b] = gradient.color(lo + t * (hi - lo));
        let slice = Rect::from_min_max(
            pos2(rect.left() + width * i as f32 / LEGEND_SLICES as f32, rect.top()),
            pos2(rect.left() + width * (i + 1) as f32 / LEGEND_SLICES as f32, rect.bottom()),
        );
        ui.painter().rect_filled(slice, 0.0, Color32::from_rgb(r, g, b));
    }
    ui.horizontal(|ui| {
        ui.label(format!("{lo:.0} {unit}"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!("{hi:.0} {unit}"));
        });
    });
}

/// The grid value under map position `(x, y)` in pixels, `None` outside or where empty.
pub fn value_at(grid: &Grid, x: f64, y: f64) -> Option<f32> {
    if !(x >= 0.0 && y >= 0.0) {
        return None;
    }
    let (col, row) = ((x / grid.cell_px) as usize, (y / grid.cell_px) as usize);
    // `Grid::get` indexes row-major without checking the column, so a point right of the
    // grid would read the first cells of the next row.
    if col >= grid.cols || row >= grid.rows {
        return None;
    }
    grid.get(col, row)
}

/// Text for a grid `value` of `metric`: the class label for categorical metrics, otherwise
/// the number with its unit.
pub fn value_label(metric: &Metric, value: f32) -> String {
    match metric.palette() {
        Some(palette) => {
            let i = (value.round().max(0.0) as usize).min(palette.entries().len() - 1);
            palette.entries()[i].0.to_owned()
        }
        // Jitter is routinely below 1 ms, where whole numbers would read as zero.
        None => format!("{value:.*} {}", usize::from(metric.unit() == "ms"), metric.unit()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid { cols: 3, rows: 2, cell_px: 10.0, values: vec![Some(1.0), Some(2.0), None, Some(4.0), Some(5.0), Some(6.0)] }
    }

    #[test]
    fn value_lookup_uses_cell_size_and_rejects_outside_points() {
        let g = grid();
        assert_eq!(value_at(&g, 5.0, 5.0), Some(1.0));
        assert_eq!(value_at(&g, 15.0, 5.0), Some(2.0));
        assert_eq!(value_at(&g, 25.0, 5.0), None, "empty cell");
        assert_eq!(value_at(&g, 5.0, 15.0), Some(4.0));
        assert_eq!(value_at(&g, 35.0, 5.0), None, "right of the grid");
        assert_eq!(value_at(&g, -1.0, 5.0), None);
        assert_eq!(value_at(&g, 5.0, 25.0), None, "below the grid");
        assert_eq!(value_at(&g, f64::NAN, 5.0), None);
    }

    #[test]
    fn labels_follow_the_metric() {
        assert_eq!(value_label(&Metric::Signal(Vec::new()), -61.4), "-61 dBm");
        assert_eq!(value_label(&Metric::ApCount { min_rssi_dbm: -80 }, 3.0), "3 APs");
        let cov = Metric::BandCoverage { wanted: Vec::new(), min_rssi_dbm: -80 };
        assert_eq!(value_label(&cov, 1.0), "2.4 GHz only");
        assert_eq!(value_label(&cov, 99.0), "Several bands", "out-of-range classes clamp to the last label");
    }
}
