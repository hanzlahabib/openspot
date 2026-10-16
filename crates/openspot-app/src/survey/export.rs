//! PNG export of the plan with exactly what the canvas shows: heatmap (with radius, crop
//! and gray-out applied) and measurement points.

use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use openspot_heatmap::Gradient;

use super::Survey;

/// Background of an export made with the floor plan hidden.
const NO_PLAN_BACKGROUND: Rgba<u8> = Rgba([255, 255, 255, 255]);
/// Radius of a point marker in the exported image, in map pixels.
const POINT_RADIUS_PX: f32 = 5.0;
/// White ring around each marker so it reads on any heat color.
const POINT_RING_PX: f32 = 2.0;

impl Survey {
    /// Encoded PNG of the current zone and snapshot.
    pub fn render_png(&self) -> Result<Vec<u8>, String> {
        let map = self.zone().map.as_ref().ok_or("this zone has no floor plan")?;
        let mut canvas = image::load_from_memory(&map.data).map_err(|e| format!("could not decode the floor plan: {e}"))?.to_rgba8();
        if !self.show_floor_plan {
            canvas = RgbaImage::from_pixel(canvas.width(), canvas.height(), NO_PLAN_BACKGROUND);
        }

        if let Some(grid) = self.compute_grid() {
            let rgba = self.heat_rgba(&grid);
            if let Some(raster) = RgbaImage::from_raw(grid.cols as u32, grid.rows as u32, rgba) {
                // The raster covers whole cells, slightly more than the map: crop before
                // scaling so cells line up with map pixels.
                let cell = grid.cell_px;
                let crop_w = ((f64::from(canvas.width()) / cell).ceil() as u32).min(raster.width());
                let crop_h = ((f64::from(canvas.height()) / cell).ceil() as u32).min(raster.height());
                let cropped = imageops::crop_imm(&raster, 0, 0, crop_w, crop_h).to_image();
                let scaled = imageops::resize(
                    &cropped,
                    (f64::from(crop_w) * cell).round() as u32,
                    (f64::from(crop_h) * cell).round() as u32,
                    FilterType::Triangle,
                );
                imageops::overlay(&mut canvas, &scaled, 0, 0);
            }
        }
        if self.show_points {
            let gradient = Gradient::signal_dbm();
            for p in &self.filtered_snapshot().points {
                let best = p.access_points.iter().map(|a| a.rssi_dbm).max().map_or(-100.0, |v| v as f32);
                let [r, g, b] = gradient.color(best);
                dot(&mut canvas, p.x as f32, p.y as f32, Rgba([r, g, b, 255]));
            }
        }
        let mut out = std::io::Cursor::new(Vec::new());
        canvas.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(out.into_inner())
    }
}

/// Filled disc with a white ring.
fn dot(img: &mut RgbaImage, cx: f32, cy: f32, fill: Rgba<u8>) {
    let r_in = POINT_RADIUS_PX - POINT_RING_PX;
    let (x0, x1) = ((cx - POINT_RADIUS_PX - 1.0).floor().max(0.0) as u32, (cx + POINT_RADIUS_PX + 1.0).ceil() as u32);
    let (y0, y1) = ((cy - POINT_RADIUS_PX - 1.0).floor().max(0.0) as u32, (cy + POINT_RADIUS_PX + 1.0).ceil() as u32);
    for y in y0..=y1.min(img.height().saturating_sub(1)) {
        for x in x0..=x1.min(img.width().saturating_sub(1)) {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d <= r_in {
                img.put_pixel(x, y, fill);
            } else if d <= POINT_RADIUS_PX {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
    }
}
