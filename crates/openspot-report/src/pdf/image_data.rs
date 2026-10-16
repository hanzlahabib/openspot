//! Raster images prepared for embedding as PDF XObjects.

use image::imageops::{self, FilterType};

use super::Doc;
use crate::error::ReportError;
use super::writer::deflate;

/// Handle to an image added to a [`Doc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageId(pub(super) usize);

impl ImageId {
    pub(super) fn resource_name(self) -> String {
        format!("Im{}", self.0)
    }
}

/// An image already converted to Flate-compressed 8-bit RGB (plus optional alpha mask).
#[derive(Debug)]
pub(super) struct PdfImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
}

impl Doc {
    /// Decodes PNG/JPEG `bytes`, shrinks it so neither side exceeds `max_side` pixels
    /// (keeping the PDF small), and registers it. Returns the id and the *original* pixel
    /// size, which callers need for aspect ratio and coordinate mapping.
    pub fn add_image(&mut self, bytes: &[u8], max_side: u32) -> Result<(ImageId, u32, u32), ReportError> {
        let mut rgba = image::load_from_memory(bytes)?.to_rgba8();
        let original = (rgba.width(), rgba.height());
        if rgba.width().max(rgba.height()) > max_side {
            let k = f64::from(max_side) / f64::from(rgba.width().max(rgba.height()));
            let (w, h) = ((f64::from(rgba.width()) * k).round().max(1.0) as u32, (f64::from(rgba.height()) * k).round().max(1.0) as u32);
            rgba = imageops::resize(&rgba, w, h, FilterType::Triangle);
        }
        let (w, h) = (rgba.width(), rgba.height());
        let has_alpha = rgba.pixels().any(|p| p[3] != 255);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        let mut alpha = Vec::with_capacity(if has_alpha { (w * h) as usize } else { 0 });
        for p in rgba.pixels() {
            rgb.extend_from_slice(&p.0[..3]);
            if has_alpha {
                alpha.push(p[3]);
            }
        }
        self.images.push(PdfImage { width: w, height: h, rgb: deflate(&rgb), alpha: has_alpha.then(|| deflate(&alpha)) });
        Ok((ImageId(self.images.len() - 1), original.0, original.1))
    }
}
