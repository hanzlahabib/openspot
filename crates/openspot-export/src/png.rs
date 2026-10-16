use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use openspot_heatmap::{GradientPreset, GridSpec, Metric, Smoothing, colorize_ranged, rasterize};
use openspot_model::{Snapshot, Zone};
use thiserror::Error;

/// Same raster resolution as the on-screen heatmap, so exports match what the user saw.
const LONG_SIDE_CELLS: f64 = 160.0;
const EXTEND_FRACTION: f64 = 0.06;
const POINT_RADIUS_PX: f32 = 5.0;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("this zone has no floor plan")]
    NoMap,
    #[error("could not decode the floor plan: {0}")]
    Decode(#[from] image::ImageError),
    #[error("contour lines need a continuous metric, not a categorical one")]
    NotContinuous,
    #[error("the heatmap raster does not match its stated size or cell size")]
    BadRaster,
}

/// Renders the floor plan with the heatmap for `metric` blended over it (`alpha` 0-255),
/// and the measurement points on top. Returns encoded PNG bytes.
pub fn render_png(
    zone: &Zone,
    snapshot: &Snapshot,
    metric: &Metric,
    preset: GradientPreset,
    smoothing: Smoothing,
    alpha: u8,
    show_points: bool,
) -> Result<Vec<u8>, ExportError> {
    let style = PngStyle { preset, range: None, smoothing, alpha, show_points };
    render_png_styled(zone, snapshot, metric, &style)
}

/// Appearance of an exported heatmap image.
#[derive(Debug, Clone, Copy)]
pub struct PngStyle {
    pub preset: GradientPreset,
    /// User value range for continuous gradients (H-15); `None` keeps the metric's own.
    pub range: Option<(f32, f32)>,
    pub smoothing: Smoothing,
    /// Heatmap opacity 0-255.
    pub alpha: u8,
    pub show_points: bool,
}

/// [`render_png`] with the full style, including a user value range.
pub fn render_png_styled(zone: &Zone, snapshot: &Snapshot, metric: &Metric, style: &PngStyle) -> Result<Vec<u8>, ExportError> {
    let PngStyle { preset, range, smoothing, alpha, show_points } = *style;
    let map = zone.map.as_ref().ok_or(ExportError::NoMap)?;
    let mut canvas = image::load_from_memory(&map.data)?.to_rgba8();
    let (w, h) = (canvas.width(), canvas.height());

    let long = f64::from(w.max(h));
    let spec = GridSpec {
        width_px: f64::from(w),
        height_px: f64::from(h),
        cell_px: long / LONG_SIDE_CELLS,
        extend_px: long * EXTEND_FRACTION,
    };
    let heat = rasterize(snapshot, metric, &spec, smoothing).and_then(|grid| {
        let rgba = colorize_ranged(metric, &grid, alpha, preset, range);
        RgbaImage::from_raw(grid.cols as u32, grid.rows as u32, rgba)
    });
    if let Some(raster) = heat {
        overlay_raster(&mut canvas, &raster, spec.cell_px);
    }

    if show_points {
        let gradient = openspot_heatmap::Gradient::signal_dbm();
        for p in &snapshot.points {
            let best = p.access_points.iter().map(|a| a.rssi_dbm).max().map_or(-100.0, |v| v as f32);
            let [r, g, b] = gradient.color(best);
            dot(&mut canvas, p.x as f32, p.y as f32, Rgba([r, g, b, 255]));
        }
    }

    let mut out = std::io::Cursor::new(Vec::new());
    canvas.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

/// Draws `raster` (one pixel per grid cell of `cell_px` map pixels) over `canvas`. The raster
/// covers `cols * cell` pixels, slightly more than the map; it is cropped before scaling so
/// cells line up with map pixels.
fn overlay_raster(canvas: &mut RgbaImage, raster: &RgbaImage, cell_px: f64) {
    let (w, h) = (canvas.width(), canvas.height());
    let crop_w = ((f64::from(w) / cell_px).ceil() as u32).min(raster.width());
    let crop_h = ((f64::from(h) / cell_px).ceil() as u32).min(raster.height());
    let cropped = imageops::crop_imm(raster, 0, 0, crop_w, crop_h).to_image();
    let scaled = imageops::resize(&cropped, (f64::from(crop_w) * cell_px).round() as u32, (f64::from(crop_h) * cell_px).round() as u32, FilterType::Triangle);
    blend_over(canvas, &scaled);
}

/// A marker drawn on an exported plan: map position in pixels and fill color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    pub x: f64,
    pub y: f64,
    pub rgb: [u8; 3],
}

/// Renders the floor plan with an already computed `raster` (RGBA, one pixel per cell of
/// `cell_px` map pixels, `cols` x `rows`) blended over it, and `markers` on top. This is how
/// predictions are exported: the caller has colored the grid exactly as shown on screen.
pub fn render_png_raster(zone: &Zone, cols: u32, rows: u32, rgba: Vec<u8>, cell_px: f64, markers: &[Marker]) -> Result<Vec<u8>, ExportError> {
    let map = zone.map.as_ref().ok_or(ExportError::NoMap)?;
    let mut canvas = image::load_from_memory(&map.data)?.to_rgba8();
    if cell_px.is_finite() && cell_px > 0.0 {
        let raster = RgbaImage::from_raw(cols, rows, rgba).ok_or(ExportError::BadRaster)?;
        overlay_raster(&mut canvas, &raster, cell_px);
    } else {
        return Err(ExportError::BadRaster);
    }
    for m in markers {
        dot(&mut canvas, m.x as f32, m.y as f32, Rgba([m.rgb[0], m.rgb[1], m.rgb[2], 255]));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    canvas.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

/// Standard source-over compositing of `top` (straight alpha) onto opaque `base`.
fn blend_over(base: &mut RgbaImage, top: &RgbaImage) {
    for (x, y, t) in top.enumerate_pixels() {
        if x >= base.width() || y >= base.height() || t[3] == 0 {
            continue;
        }
        let a = u32::from(t[3]);
        let b = base.get_pixel_mut(x, y);
        for k in 0..3 {
            b[k] = ((u32::from(t[k]) * a + u32::from(b[k]) * (255 - a)) / 255) as u8;
        }
    }
}

/// Filled disc with a white ring.
fn dot(img: &mut RgbaImage, cx: f32, cy: f32, fill: Rgba<u8>) {
    let r_out = POINT_RADIUS_PX;
    let r_in = POINT_RADIUS_PX - 2.0;
    let (x0, x1) = ((cx - r_out - 1.0).floor().max(0.0) as u32, (cx + r_out + 1.0).ceil() as u32);
    let (y0, y1) = ((cy - r_out - 1.0).floor().max(0.0) as u32, (cy + r_out + 1.0).ceil() as u32);
    for y in y0..=y1.min(img.height().saturating_sub(1)) {
        for x in x0..=x1.min(img.width().saturating_sub(1)) {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d <= r_in {
                img.put_pixel(x, y, fill);
            } else if d <= r_out {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::{MapImage, ScanPoint};
    use openspot_wlan::{AccessPoint, MacAddress};

    fn white_map(w: u32, h: u32) -> MapImage {
        let mut bytes = std::io::Cursor::new(Vec::new());
        RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255])).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        MapImage { original_name: "m.png".into(), data: bytes.into_inner(), width: w, height: h }
    }

    fn ap(rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, 1]), rssi, 2_437_000, true, 100, vec![])
    }

    fn zone_with_points() -> (Zone, Snapshot) {
        let mut zone = Zone::new("z");
        zone.map = Some(white_map(200, 100));
        let mut snap = Snapshot::new("s");
        for (x, y, r) in [(20.0, 20.0, -40), (180.0, 20.0, -60), (100.0, 90.0, -85)] {
            snap.points.push(ScanPoint::new(x, y, 0, vec![ap(r)]));
        }
        (zone, snap)
    }

    #[test]
    fn png_has_map_dimensions_and_colored_overlay() {
        let (zone, snap) = zone_with_points();
        let bytes = render_png(&zone, &snap, &Metric::Signal(vec![]), GradientPreset::Default, Smoothing::Off, 200, false).unwrap();
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height()), (200, 100));
        // A pixel near the strong point is no longer white; one far outside the data is.
        assert_ne!(img.get_pixel(25, 25), &Rgba([255, 255, 255, 255]));
        assert_eq!(img.get_pixel(199, 99), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn points_are_drawn_when_requested() {
        let (zone, snap) = zone_with_points();
        let with = render_png(&zone, &snap, &Metric::Signal(vec![]), GradientPreset::Default, Smoothing::Off, 0, true).unwrap();
        let without = render_png(&zone, &snap, &Metric::Signal(vec![]), GradientPreset::Default, Smoothing::Off, 0, false).unwrap();
        assert_ne!(with, without);
        let img = image::load_from_memory(&with).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(20, 20)[3], 255);
    }

    #[test]
    fn raster_export_puts_each_cell_where_the_grid_says_and_draws_markers_on_top() {
        let mut zone = Zone::new("z");
        zone.map = Some(white_map(200, 100));
        // 4 x 2 cells of 50 px; cell (1, 0) red, the rest transparent.
        let mut rgba = vec![0u8; 4 * 2 * 4];
        rgba[4..8].copy_from_slice(&[255, 0, 0, 255]);
        let marker = Marker { x: 175.0, y: 75.0, rgb: [0, 0, 255] };
        let bytes = render_png_raster(&zone, 4, 2, rgba, 50.0, &[marker]).unwrap();
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height()), (200, 100));
        // The raster is smoothed when scaled up (as for survey exports), so the cell center is
        // red to within a few levels.
        let red = img.get_pixel(75, 25);
        assert!(red[0] > 240 && red[1] < 20 && red[2] < 20, "inside cell (1,0): {red:?}");
        assert_eq!(img.get_pixel(175, 25), &Rgba([255, 255, 255, 255]), "a transparent cell far from the red one leaves the plan untouched");
        assert_eq!(img.get_pixel(175, 75), &Rgba([0, 0, 255, 255]), "marker fill");
    }

    #[test]
    fn raster_export_rejects_mismatched_rasters_and_missing_plans() {
        let mut zone = Zone::new("z");
        assert!(matches!(render_png_raster(&zone, 1, 1, vec![0; 4], 10.0, &[]), Err(ExportError::NoMap)));
        zone.map = Some(white_map(40, 40));
        assert!(matches!(render_png_raster(&zone, 2, 2, vec![0; 4], 10.0, &[]), Err(ExportError::BadRaster)), "pixel count does not match 2 x 2");
        assert!(matches!(render_png_raster(&zone, 1, 1, vec![0; 4], 0.0, &[]), Err(ExportError::BadRaster)));
        assert!(matches!(render_png_raster(&zone, 1, 1, vec![0; 4], f64::NAN, &[]), Err(ExportError::BadRaster)));
    }

    #[test]
    fn missing_map_and_empty_snapshot() {
        let zone = Zone::new("no map");
        assert!(matches!(render_png(&zone, &Snapshot::new("s"), &Metric::Signal(vec![]), GradientPreset::Default, Smoothing::Off, 100, true), Err(ExportError::NoMap)));
        let mut z2 = Zone::new("z");
        z2.map = Some(white_map(40, 40));
        let png = render_png(&z2, &Snapshot::new("empty"), &Metric::Signal(vec![]), GradientPreset::Default, Smoothing::Off, 100, true).unwrap();
        assert!(!png.is_empty());
    }
}
