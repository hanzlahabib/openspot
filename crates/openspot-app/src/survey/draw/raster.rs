//! Rendering a [`Drawing`] to pixels.

use image::{Rgba, RgbaImage};
use openspot_model::{Drawing, Shape, ShapeKind};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use super::text::{font, layout};
use super::{DrawError, MAX_DRAWING_SIDE_PX};
use ab_glyph::Font;

/// Dark gray rather than pure black: softer on screen, still maximum-contrast for a plan.
pub(super) const INK: [u8; 3] = [30, 30, 30];
const PAPER: [u8; 3] = [255, 255, 255];

fn paint() -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(INK[0], INK[1], INK[2], 255);
    p.anti_alias = true;
    p
}

/// Renders the drawing on white paper at its own pixel size.
pub fn render(d: &Drawing) -> Result<RgbaImage, DrawError> {
    if !(1..=MAX_DRAWING_SIDE_PX).contains(&d.width) || !(1..=MAX_DRAWING_SIDE_PX).contains(&d.height) {
        return Err(DrawError::BadSize);
    }
    let mut pixmap = Pixmap::new(d.width, d.height).ok_or(DrawError::BadSize)?;
    pixmap.fill(tiny_skia::Color::from_rgba8(PAPER[0], PAPER[1], PAPER[2], 255));
    for shape in &d.shapes {
        draw_vector(&mut pixmap, shape);
    }
    // Paper is opaque, so premultiplied and straight alpha coincide.
    let mut img = RgbaImage::from_raw(d.width, d.height, pixmap.take()).ok_or(DrawError::BadSize)?;
    for shape in &d.shapes {
        if let ShapeKind::Text { at, text } = &shape.kind {
            draw_text(&mut img, *at, text, shape.font.px())?;
        }
    }
    Ok(img)
}

pub fn render_png(d: &Drawing) -> Result<Vec<u8>, DrawError> {
    let img = render(d)?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

fn draw_vector(pixmap: &mut Pixmap, shape: &Shape) {
    let stroke = Stroke { width: shape.thickness.px() as f32, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
    match &shape.kind {
        ShapeKind::Polyline { points } => {
            let mut pb = PathBuilder::new();
            for (i, p) in points.iter().enumerate() {
                if i == 0 {
                    pb.move_to(p.0 as f32, p.1 as f32);
                } else {
                    pb.line_to(p.0 as f32, p.1 as f32);
                }
            }
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &paint(), &stroke, Transform::identity(), None);
            }
        }
        ShapeKind::Rect { min, max, filled } => {
            let Some(rect) = Rect::from_ltrb(min.0 as f32, min.1 as f32, max.0 as f32, max.1 as f32) else { return };
            let path = PathBuilder::from_rect(rect);
            if *filled {
                pixmap.fill_path(&path, &paint(), FillRule::Winding, Transform::identity(), None);
            } else {
                pixmap.stroke_path(&path, &paint(), &stroke, Transform::identity(), None);
            }
        }
        ShapeKind::Text { .. } => {}
    }
}

fn draw_text(img: &mut RgbaImage, at: (f64, f64), text: &str, px: f32) -> Result<(), DrawError> {
    let font = font()?;
    for mut glyph in layout(text, px)?.glyphs {
        glyph.position.x += at.0 as f32;
        glyph.position.y += at.1 as f32;
        let Some(outlined) = font.outline_glyph(glyph) else { continue };
        let bounds = outlined.px_bounds();
        outlined.draw(|gx, gy, coverage| {
            let (x, y) = (bounds.min.x as i64 + i64::from(gx), bounds.min.y as i64 + i64::from(gy));
            let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else { return };
            if x < img.width() && y < img.height() {
                let dst = img.get_pixel_mut(x, y);
                for (k, ink) in INK.iter().enumerate() {
                    let d = f32::from(dst[k]);
                    dst[k] = (d + (f32::from(*ink) - d) * coverage.clamp(0.0, 1.0)).round() as u8;
                }
                *dst = Rgba([dst[0], dst[1], dst[2], 255]);
            }
        });
    }
    Ok(())
}
