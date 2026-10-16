//! PDF floor plans (V-08): render one page to a raster with a pure-Rust renderer.
//!
//! `hayro` needs no native library, so the same build works on Windows and Linux without
//! bundling or downloading binaries. Its newer releases require a newer compiler than this
//! workspace's 1.88, so the dependency is pinned to the last compatible version. Known
//! renderer limits (encrypted files, some blend modes) surface as an error, never a blank plan.

mod control;

use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderSettings, render};
use image::RgbaImage;
use thiserror::Error;

/// PDF user-space units per inch.
const POINTS_PER_INCH: f32 = 72.0;
/// Resolution plans are rendered at: sharp enough to read door labels when zoomed in,
/// while a typical A1 sheet stays within memory limits.
pub const RENDER_DPI: f32 = 150.0;
/// Longest rendered edge in pixels. Equals the largest texture the canvas uploads
/// unscaled, and stays within the renderer's 16-bit pixmap size.
pub const MAX_RENDER_SIDE_PX: f32 = 4096.0;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PdfError {
    #[error("could not read the file: {0}")]
    Read(String),
    #[error("this is not a readable PDF (it may be damaged or password protected)")]
    Invalid,
    #[error("the PDF has no pages")]
    NoPages,
    #[error("the PDF has {count} pages; page {requested} does not exist")]
    NoSuchPage { requested: usize, count: usize },
    #[error("the page has no usable size")]
    BadPageSize,
    #[error("could not encode the plan image: {0}")]
    Encode(String),
}

/// A page rendered to a PNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub page_count: usize,
}

/// Pixels per PDF point for a page of `width_pt` x `height_pt`: [`RENDER_DPI`], reduced if
/// the long edge would exceed [`MAX_RENDER_SIDE_PX`].
pub fn render_scale(width_pt: f32, height_pt: f32) -> f32 {
    let wanted = RENDER_DPI / POINTS_PER_INCH;
    let long = width_pt.max(height_pt);
    if long * wanted > MAX_RENDER_SIDE_PX { MAX_RENDER_SIDE_PX / long } else { wanted }
}

fn open(data: Vec<u8>) -> Result<Pdf, PdfError> {
    Pdf::new(Arc::new(data)).map_err(|_| PdfError::Invalid)
}

/// Number of pages in the PDF in `data`.
pub fn page_count(data: &[u8]) -> Result<usize, PdfError> {
    Ok(open(data.to_vec())?.pages().len())
}

/// Renders page `page` (0-based) on a white background.
pub fn render_page(data: Vec<u8>, page: usize) -> Result<RenderedPage, PdfError> {
    let pdf = open(data)?;
    let pages = pdf.pages();
    let count = pages.len();
    if count == 0 {
        return Err(PdfError::NoPages);
    }
    let page_ref = pages.get(page).ok_or(PdfError::NoSuchPage { requested: page + 1, count })?;
    let (w_pt, h_pt) = page_ref.render_dimensions();
    if !(w_pt.is_finite() && h_pt.is_finite() && w_pt >= 1.0 && h_pt >= 1.0) {
        return Err(PdfError::BadPageSize);
    }
    let scale = render_scale(w_pt, h_pt);
    let settings = RenderSettings { x_scale: scale, y_scale: scale, bg_color: WHITE, ..RenderSettings::default() };
    let pixmap = render(page_ref, &InterpreterSettings::default(), &settings);
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    // The white background makes every pixel opaque, so premultiplied equals straight alpha.
    let img = RgbaImage::from_raw(width, height, pixmap.data_as_u8_slice().to_vec()).ok_or(PdfError::BadPageSize)?;
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).map_err(|e| PdfError::Encode(e.to_string()))?;
    Ok(RenderedPage { png: png.into_inner(), width, height, page_count: count })
}

pub use control::PdfChoice;

#[cfg(test)]
pub(crate) mod tests;
