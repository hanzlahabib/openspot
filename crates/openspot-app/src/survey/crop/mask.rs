//! Pure raster masks: crop the interpolated grid, tint unmeasured cells gray.

use openspot_heatmap::Grid;
use openspot_model::CropArea;

/// Neutral mid-gray: reads as "no data" on both light and dark floor plans and does not
/// collide with any gradient color.
pub const GRAY_RGB: [u8; 3] = [128, 128, 128];

/// Where measurements exist: a cell is measured if a sample lies within `radius_px`.
#[derive(Debug, Clone, Copy)]
pub struct GrayOut<'a> {
    pub samples: &'a [(f64, f64)],
    pub radius_px: f64,
}

fn cell_center(grid_cell_px: f64, col: usize, row: usize) -> (f64, f64) {
    ((col as f64 + 0.5) * grid_cell_px, (row as f64 + 0.5) * grid_cell_px)
}

/// Empties every cell whose center lies outside `crop`.
pub fn crop_grid(grid: &mut Grid, crop: &CropArea) {
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let (x, y) = cell_center(grid.cell_px, col, row);
            if !crop.contains(x, y) {
                grid.values[row * grid.cols + col] = None;
            }
        }
    }
}

/// Paints cells with no sample within the radius gray (opacity `alpha`), replacing
/// whatever color was there. Cells outside `crop` (if any) are left untouched, so a crop
/// still limits where anything is drawn. `rgba` is `cols * rows` pixels as produced by the
/// heatmap colorizer.
pub fn tint_unmeasured(rgba: &mut [u8], grid: &Grid, crop: Option<&CropArea>, gray: GrayOut<'_>, alpha: u8) {
    let r2 = gray.radius_px * gray.radius_px;
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let (x, y) = cell_center(grid.cell_px, col, row);
            if crop.is_some_and(|c| !c.contains(x, y)) {
                continue;
            }
            let measured = gray.samples.iter().any(|s| (s.0 - x).powi(2) + (s.1 - y).powi(2) <= r2);
            if !measured {
                let i = (row * grid.cols + col) * 4;
                if let Some(px) = rgba.get_mut(i..i + 4) {
                    px.copy_from_slice(&[GRAY_RGB[0], GRAY_RGB[1], GRAY_RGB[2], alpha]);
                }
            }
        }
    }
}
