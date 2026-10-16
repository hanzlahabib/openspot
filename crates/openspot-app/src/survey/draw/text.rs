//! Text layout with the bundled font (Lato, SIL Open Font License; see assets/fonts/OFL.txt).

use std::sync::LazyLock;

use ab_glyph::{Font, FontRef, Glyph, InvalidFont, PxScale, ScaleFont, point};

use super::DrawError;

static FONT: LazyLock<Result<FontRef<'static>, InvalidFont>> =
    LazyLock::new(|| FontRef::try_from_slice(include_bytes!("../../../assets/fonts/Lato-Regular.ttf")));

pub(super) fn font() -> Result<&'static FontRef<'static>, DrawError> {
    FONT.as_ref().map_err(|_| DrawError::Font)
}

/// Glyphs positioned relative to the text's top-left corner, plus the text's size.
pub(super) struct Layout {
    pub glyphs: Vec<Glyph>,
    pub width: f32,
    pub height: f32,
}

/// Lays `text` out at font height `px`; `\n` starts a new line.
pub(super) fn layout(text: &str, px: f32) -> Result<Layout, DrawError> {
    let font = font()?;
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let line_height = scaled.ascent() - scaled.descent() + scaled.line_gap();
    let mut glyphs = Vec::new();
    let mut width = 0.0f32;
    let mut lines = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        lines += 1;
        let baseline = i as f32 * line_height + scaled.ascent();
        let mut x = 0.0f32;
        let mut prev = None;
        for c in line.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                x += scaled.kern(p, id);
            }
            glyphs.push(id.with_scale_and_position(scale, point(x, baseline)));
            x += scaled.h_advance(id);
            prev = Some(id);
        }
        width = width.max(x);
    }
    Ok(Layout { glyphs, width, height: lines as f32 * line_height })
}

/// Size `(width, height)` in map pixels that `text` occupies at font height `px`.
pub fn text_size(text: &str, px: f32) -> Option<(f64, f64)> {
    layout(text, px).ok().map(|l| (f64::from(l.width), f64::from(l.height)))
}
