//! Minimal PDF 1.7 page builder on top of `pdf-writer`.
//!
//! Why not `printpdf`/`genpdf`: see `docs/REPORTS.md`. In short, this keeps the output
//! byte-for-byte deterministic, makes text extractable (every embedded font carries a
//! `ToUnicode` map), and avoids pulling a layout/HTML engine for what is a fixed set of
//! pages. All coordinates passed to drawing methods are in points with the origin at the
//! top-left of the page and y growing downwards.

mod font;
mod image_data;
mod writer;

pub use font::{FontFace, Weight, truncate, wrap};
pub use image_data::ImageId;
pub use writer::Metadata;

use std::collections::{BTreeMap, BTreeSet};

use pdf_writer::{Content, Name, Str};

use crate::error::ReportError;

/// A4 in points (1 pt = 1/72 in): 210 x 297 mm.
pub const PAGE_W: f32 = 595.276;
pub const PAGE_H: f32 = 841.89;

pub type Rgb = [u8; 3];

/// Bezier control-point offset that makes four cubic segments approximate a circle
/// (4/3 * (sqrt(2) - 1)).
const CIRCLE_KAPPA: f32 = 0.552_284_8;

struct FontState {
    face: FontFace,
    /// Glyphs drawn so far, for the `ToUnicode` map and width table.
    used: BTreeMap<u16, char>,
}

struct PageData {
    content: Content,
    images: BTreeSet<usize>,
}

#[derive(Debug)]
pub struct Doc {
    fonts: [FontState; 2],
    images: Vec<image_data::PdfImage>,
    pages: Vec<PageData>,
    cur: usize,
}

impl std::fmt::Debug for FontState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontState").field("glyphs", &self.used.len()).finish()
    }
}

impl std::fmt::Debug for PageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageData").field("images", &self.images).finish()
    }
}

fn unit(c: Rgb) -> (f32, f32, f32) {
    (f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0)
}

impl Doc {
    /// A document with one blank page.
    pub fn new() -> Result<Self, ReportError> {
        let font = |w| -> Result<FontState, ReportError> { Ok(FontState { face: FontFace::load(w)?, used: BTreeMap::new() }) };
        let mut doc = Self {
            fonts: [font(Weight::Regular)?, font(Weight::Bold)?],
            images: Vec::new(),
            pages: Vec::new(),
            cur: 0,
        };
        doc.new_page();
        Ok(doc)
    }

    pub fn new_page(&mut self) {
        self.pages.push(PageData { content: Content::new(), images: BTreeSet::new() });
        self.cur = self.pages.len() - 1;
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Directs subsequent drawing to page `index` (0-based), e.g. to add footers last.
    pub fn select_page(&mut self, index: usize) {
        assert!(index < self.pages.len(), "page {index} does not exist");
        self.cur = index;
    }

    pub fn face(&self, weight: Weight) -> &FontFace {
        &self.fonts[weight as usize].face
    }

    pub fn sanitize(&self, text: &str) -> String {
        self.face(Weight::Regular).sanitize(text)
    }

    /// Width in points of `text` as it would be drawn.
    pub fn measure(&self, weight: Weight, size: f32, text: &str) -> f32 {
        let face = self.face(weight);
        face.width(&face.sanitize(text), size)
    }

    fn page(&mut self) -> &mut PageData {
        &mut self.pages[self.cur]
    }

    /// Draws `text` with its baseline at `y` and left edge at `x`; returns the width.
    pub fn text(&mut self, x: f32, y: f32, size: f32, weight: Weight, color: Rgb, text: &str) -> f32 {
        let state = &mut self.fonts[weight as usize];
        let clean = state.face.sanitize(text);
        if clean.is_empty() {
            return 0.0;
        }
        // Identity-H: each character is the two-byte glyph id of its glyph.
        let mut bytes = Vec::with_capacity(clean.len() * 2);
        for c in clean.chars() {
            // `sanitize` guarantees a glyph exists (the replacement glyph is in the subset).
            let gid = state.face.glyph(c).map_or(0, |g| g.0);
            state.used.entry(gid).or_insert(c);
            bytes.extend_from_slice(&gid.to_be_bytes());
        }
        let width = state.face.width(&clean, size);
        let (r, g, b) = unit(color);
        let name = if weight == Weight::Regular { b"F1" } else { b"F2" };
        let c = &mut self.page().content;
        c.set_fill_rgb(r, g, b).begin_text().set_font(Name(name), size).next_line(x, PAGE_H - y).show(Str(&bytes)).end_text();
        width
    }

    /// Like [`Doc::text`] but right-aligned to `x_right`.
    pub fn text_right(&mut self, x_right: f32, y: f32, size: f32, weight: Weight, color: Rgb, text: &str) -> f32 {
        let w = self.measure(weight, size, text);
        self.text(x_right - w, y, size, weight, color, text)
    }

    /// Like [`Doc::text`] but centred on `x_center`.
    pub fn text_centered(&mut self, x_center: f32, y: f32, size: f32, weight: Weight, color: Rgb, text: &str) -> f32 {
        let w = self.measure(weight, size, text);
        self.text(x_center - w / 2.0, y, size, weight, color, text)
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        let (r, g, b) = unit(color);
        self.page().content.set_fill_rgb(r, g, b).rect(x, PAGE_H - y - h, w, h).fill_nonzero();
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, line_width: f32, color: Rgb) {
        let (r, g, b) = unit(color);
        self.page().content.set_stroke_rgb(r, g, b).set_line_width(line_width).rect(x, PAGE_H - y - h, w, h).stroke();
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, line_width: f32, color: Rgb) {
        self.polyline(&[(x1, y1), (x2, y2)], line_width, color);
    }

    /// Open path through `points`; fewer than two points draw nothing.
    pub fn polyline(&mut self, points: &[(f32, f32)], line_width: f32, color: Rgb) {
        let Some((first, rest)) = points.split_first().filter(|(_, rest)| !rest.is_empty()) else { return };
        let (r, g, b) = unit(color);
        let c = &mut self.page().content;
        c.set_stroke_rgb(r, g, b).set_line_width(line_width).move_to(first.0, PAGE_H - first.1);
        for p in rest {
            c.line_to(p.0, PAGE_H - p.1);
        }
        c.stroke();
    }

    /// Closed, filled polygon with an optional outline.
    pub fn polygon(&mut self, points: &[(f32, f32)], fill: Rgb, outline: Option<(f32, Rgb)>) {
        let Some((first, rest)) = points.split_first().filter(|(_, rest)| rest.len() >= 2) else { return };
        let (r, g, b) = unit(fill);
        let c = &mut self.page().content;
        c.set_fill_rgb(r, g, b).move_to(first.0, PAGE_H - first.1);
        for p in rest {
            c.line_to(p.0, PAGE_H - p.1);
        }
        c.close_path();
        match outline {
            Some((w, col)) => {
                let (sr, sg, sb) = unit(col);
                c.set_stroke_rgb(sr, sg, sb).set_line_width(w).fill_nonzero_and_stroke();
            }
            None => {
                c.fill_nonzero();
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, fill: Rgb, outline: Option<(f32, Rgb)>) {
        let (r, g, b) = unit(fill);
        let k = radius * CIRCLE_KAPPA;
        let (cy, rad) = (PAGE_H - cy, radius);
        let c = &mut self.page().content;
        c.set_fill_rgb(r, g, b)
            .move_to(cx + rad, cy)
            .cubic_to(cx + rad, cy + k, cx + k, cy + rad, cx, cy + rad)
            .cubic_to(cx - k, cy + rad, cx - rad, cy + k, cx - rad, cy)
            .cubic_to(cx - rad, cy - k, cx - k, cy - rad, cx, cy - rad)
            .cubic_to(cx + k, cy - rad, cx + rad, cy - k, cx + rad, cy)
            .close_path();
        match outline {
            Some((w, col)) => {
                let (sr, sg, sb) = unit(col);
                c.set_stroke_rgb(sr, sg, sb).set_line_width(w).fill_nonzero_and_stroke();
            }
            None => {
                c.fill_nonzero();
            }
        }
    }

    /// Places an image with its top-left corner at `(x, y)`, scaled to `w` x `h`.
    pub fn image(&mut self, id: ImageId, x: f32, y: f32, w: f32, h: f32) {
        let name = id.resource_name();
        let page = self.page();
        page.images.insert(id.0);
        page.content
            .save_state()
            .transform([w, 0.0, 0.0, h, x, PAGE_H - y - h])
            .x_object(Name(name.as_bytes()))
            .restore_state();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testpdf::Parsed;

    fn meta(created: i64) -> Metadata<'static> {
        Metadata { title: "Titel \u{e9}", author: "A", subject: "S", keywords: "k", creator: "OpenSpot", producer: "p", created_unix: created }
    }

    fn png(w: u32, h: u32, alpha: u8) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 90, alpha]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn text_round_trips_through_embedded_fonts() {
        let mut doc = Doc::new().unwrap();
        doc.text(50.0, 100.0, 12.0, Weight::Regular, [0, 0, 0], "Caf\u{e9} Z\u{fc}rich \u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}");
        doc.text(50.0, 120.0, 12.0, Weight::Bold, [0, 0, 0], "SSID \u{65e5}\u{672c}");
        doc.new_page();
        doc.text(50.0, 100.0, 12.0, Weight::Regular, [0, 0, 0], "second page");
        let bytes = doc.finish(&meta(0));

        let p = Parsed::new(&bytes);
        assert_eq!(p.page_count(), 2);
        assert_eq!(p.page_runs(p.page_ids()[0]), vec!["Caf\u{e9} Z\u{fc}rich \u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}", "SSID \u{fffd}\u{fffd}"]);
        assert_eq!(p.page_text(1), "second page");
        assert_eq!(p.info(b"Title").as_deref(), Some("Titel \u{e9}"));
        assert_eq!(p.info(b"Producer").as_deref(), Some("p"));
        assert!(p.info(b"CreationDate").unwrap().starts_with("D:19700101000000"));
    }

    #[test]
    fn images_embed_with_alpha_mask_only_when_needed() {
        let mut doc = Doc::new().unwrap();
        let (opaque, w, h) = doc.add_image(&png(40, 20, 255), 1000).unwrap();
        assert_eq!((w, h), (40, 20));
        let (translucent, ..) = doc.add_image(&png(40, 20, 128), 1000).unwrap();
        doc.image(opaque, 10.0, 10.0, 80.0, 40.0);
        doc.image(translucent, 10.0, 60.0, 80.0, 40.0);
        let bytes = doc.finish(&meta(0));
        let p = Parsed::new(&bytes);
        // opaque (1) + translucent (1) + its soft mask (1)
        assert_eq!(p.image_objects().len(), 3);
        assert_eq!(p.op_count(p.page_ids()[0], "Do"), 2);
        let masked = p.image_objects().into_iter().filter(|s| s.dict.has(b"SMask")).count();
        assert_eq!(masked, 1);
    }

    #[test]
    fn large_images_are_downscaled() {
        let mut doc = Doc::new().unwrap();
        let (_, w, h) = doc.add_image(&png(400, 200, 255), 100).unwrap();
        assert_eq!((w, h), (400, 200), "the original size is reported");
        let bytes = doc.finish(&meta(0));
        let p = Parsed::new(&bytes);
        let img = p.image_objects()[0];
        assert_eq!((img.dict.get(b"Width").unwrap().as_i64().unwrap(), img.dict.get(b"Height").unwrap().as_i64().unwrap()), (100, 50));
    }

    #[test]
    fn output_is_deterministic_and_changes_with_content() {
        let build = |t: &str| {
            let mut doc = Doc::new().unwrap();
            doc.text(10.0, 20.0, 10.0, Weight::Regular, [1, 2, 3], t);
            doc.fill_rect(10.0, 30.0, 50.0, 5.0, [200, 0, 0]);
            doc.circle(100.0, 100.0, 5.0, [0, 0, 255], Some((1.0, [255, 255, 255])));
            doc.polyline(&[(0.0, 0.0), (50.0, 50.0), (80.0, 20.0)], 1.0, [0, 0, 0]);
            doc.finish(&meta(1_700_000_000))
        };
        assert_eq!(build("same"), build("same"));
        assert_ne!(build("same"), build("different"));
    }
}
