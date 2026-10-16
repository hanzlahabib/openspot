//! Survey-level crop and gray-out controls.

use openspot_heatmap::{Grid, GradientPreset};
use openspot_model::CropArea;

use super::{CropMode, GrayOut, crop_grid, tint_unmeasured};
use crate::survey::Survey;

impl Survey {
    pub fn crop(&self) -> Option<&CropArea> {
        self.project.canvas.zone(self.zone().id).and_then(|z| z.crop.as_ref())
    }

    pub fn gray_unmeasured(&self) -> bool {
        self.project.canvas.zone(self.zone().id).is_some_and(|z| z.gray_unmeasured)
    }

    pub fn set_crop(&mut self, crop: Option<CropArea>) {
        let id = self.zone().id;
        if self.project.canvas.zone(id).and_then(|z| z.crop.as_ref()) != crop.as_ref() {
            self.project.canvas.zone_mut(id).crop = crop;
            self.touch();
        }
    }

    pub fn set_gray_unmeasured(&mut self, on: bool) {
        if on != self.gray_unmeasured() {
            let id = self.zone().id;
            self.project.canvas.zone_mut(id).gray_unmeasured = on;
            self.touch();
        }
    }

    /// Handles a map click in crop mode; a finished rectangle becomes the crop.
    pub fn crop_click(&mut self, x: f64, y: f64) {
        if let Some(area) = self.crop_draft.click((x, y)) {
            self.set_crop(Some(area));
        }
    }

    /// Closes the polygon being drawn into the crop. Returns whether it was accepted.
    pub fn crop_finish_polygon(&mut self) -> bool {
        match self.crop_draft.finish() {
            Some(area) => {
                self.set_crop(Some(area));
                true
            }
            None => false,
        }
    }

    pub fn crop_set_mode(&mut self, mode: CropMode) {
        self.crop_draft.set_mode(mode);
    }

    /// Limits an interpolated grid to the crop area (no-op without one).
    pub(crate) fn crop_interpolated(&self, grid: &mut Grid) {
        if let Some(crop) = self.crop() {
            crop_grid(grid, crop);
        }
    }

    /// Heatmap pixels for `grid`: metric colors, with unmeasured cells grayed out when
    /// that is enabled. The crop is already applied to the grid (cropped cells are empty
    /// and stay transparent).
    pub(crate) fn heat_rgba(&self, grid: &Grid) -> Vec<u8> {
        let mut rgba = self.colorize(grid, self.gradient);
        if self.gray_unmeasured() {
            let samples: Vec<(f64, f64)> = self.filtered_snapshot().points.iter().map(|p| (p.x, p.y)).collect();
            tint_unmeasured(&mut rgba, grid, self.crop(), GrayOut { samples: &samples, radius_px: self.influence_radius_px() }, self.opacity);
        }
        rgba
    }

    fn colorize(&self, grid: &Grid, preset: GradientPreset) -> Vec<u8> {
        openspot_heatmap::colorize_ranged(&self.metric(), grid, self.opacity, preset, self.active_range())
    }
}
