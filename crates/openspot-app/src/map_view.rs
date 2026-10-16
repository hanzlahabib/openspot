//! Floor-plan canvas pieces shared by Survey and Planning.

use eframe::egui::ColorImage;
use openspot_model::MapImage;

/// Largest texture side uploaded to the GPU; 4096 is supported by essentially all
/// hardware. Bigger floor plans are downscaled for display only (coordinates stay in
/// original pixels).
pub const MAX_TEXTURE_SIDE: u32 = 4096;

/// Zoom per scrolled point: about 15 % per mouse-wheel notch.
const ZOOM_PER_SCROLL_POINT: f32 = 0.0015;

/// Decodes a floor plan for display, downscaling it to [`MAX_TEXTURE_SIDE`] if needed.
pub fn decode_map(map: &MapImage) -> Result<ColorImage, String> {
    let mut img = image::load_from_memory(&map.data).map_err(|e| format!("could not decode floor plan: {e}"))?;
    if img.width().max(img.height()) > MAX_TEXTURE_SIDE {
        img = img.resize(MAX_TEXTURE_SIDE, MAX_TEXTURE_SIDE, image::imageops::FilterType::Triangle);
    }
    let rgba = img.to_rgba8();
    Ok(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// Zoom factor for one frame of mouse-wheel `scroll` points and a trackpad `pinch` factor.
pub fn scroll_zoom_factor(scroll: f32, pinch: f32) -> f32 {
    (scroll * ZOOM_PER_SCROLL_POINT).exp() * pinch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_zoom_is_symmetric_and_pinch_multiplies() {
        assert_eq!(scroll_zoom_factor(0.0, 1.0), 1.0);
        assert!((scroll_zoom_factor(100.0, 1.0) * scroll_zoom_factor(-100.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((scroll_zoom_factor(0.0, 1.5) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn large_plans_are_downscaled_but_small_ones_keep_their_size() {
        let png = |w: u32, h: u32| {
            let mut data = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(w, h, image::Rgba([1, 2, 3, 255])).write_to(&mut data, image::ImageFormat::Png).unwrap();
            MapImage { original_name: "t".into(), data: data.into_inner(), width: w, height: h }
        };
        assert_eq!(decode_map(&png(40, 20)).unwrap().size, [40, 20]);
        let big = decode_map(&png(MAX_TEXTURE_SIDE + 100, 10)).unwrap();
        assert_eq!(big.size[0], MAX_TEXTURE_SIDE as usize);
        assert!(decode_map(&MapImage { original_name: "x".into(), data: vec![1, 2, 3], width: 1, height: 1 }).is_err());
    }
}
