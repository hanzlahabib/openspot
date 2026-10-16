//! The bundled report typeface and text preparation.
//!
//! Reports embed Noto Sans (SIL OFL 1.1, see `fonts/OFL.txt`), subset to Latin, Greek,
//! Cyrillic and common punctuation, so a PDF looks the same on every machine and never
//! depends on system fonts. Text outside that coverage (CJK, Arabic, emoji, ...) cannot
//! be drawn; [`FontFace::sanitize`] replaces each such character with U+FFFD so layout
//! stays intact. The BSSID column always identifies the access point regardless.

use ttf_parser::{Face, GlyphId};
use unicode_normalization::UnicodeNormalization;

use crate::error::ReportError;

const REGULAR_TTF: &[u8] = include_bytes!("../../fonts/NotoSans-Regular.subset.ttf");
const BOLD_TTF: &[u8] = include_bytes!("../../fonts/NotoSans-Bold.subset.ttf");

/// Drawn in place of characters the bundled font cannot represent.
pub const REPLACEMENT: char = '\u{FFFD}';

/// Zero-width and bidirectional formatting characters: invisible, and the font has no
/// glyph for them, so they are dropped rather than shown as replacement boxes.
const INVISIBLE: &[char] = &[
    '\u{200B}', '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}',
    '\u{202D}', '\u{202E}', '\u{2060}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{FEFF}',
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weight {
    Regular,
    Bold,
}

pub struct FontFace {
    face: Face<'static>,
    data: &'static [u8],
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFace").field("bytes", &self.data.len()).finish()
    }
}

impl FontFace {
    pub fn load(weight: Weight) -> Result<Self, ReportError> {
        let data = match weight {
            Weight::Regular => REGULAR_TTF,
            Weight::Bold => BOLD_TTF,
        };
        Ok(Self { face: Face::parse(data, 0)?, data })
    }

    pub fn data(&self) -> &'static [u8] {
        self.data
    }

    pub fn glyph(&self, c: char) -> Option<GlyphId> {
        self.face.glyph_index(c)
    }

    /// Font units per em, as a float for scaling.
    pub fn units_per_em(&self) -> f32 {
        f32::from(self.face.units_per_em())
    }

    /// Advance of `c` in 1/1000 em (the PDF glyph-space unit).
    pub fn advance_1000(&self, c: char) -> f32 {
        let id = self.glyph(c).or_else(|| self.glyph(REPLACEMENT));
        let adv = id.and_then(|g| self.face.glyph_hor_advance(g)).unwrap_or(0);
        f32::from(adv) * 1000.0 / self.units_per_em()
    }

    /// Descriptor metrics scaled to 1/1000 em: `(ascent, descent, cap_height, bbox)`.
    pub fn metrics_1000(&self) -> (f32, f32, f32, [f32; 4]) {
        let k = 1000.0 / self.units_per_em();
        let bb = self.face.global_bounding_box();
        let cap = self.face.capital_height().map_or(f32::from(self.face.ascender()), f32::from);
        (
            f32::from(self.face.ascender()) * k,
            f32::from(self.face.descender()) * k,
            cap * k,
            [f32::from(bb.x_min) * k, f32::from(bb.y_min) * k, f32::from(bb.x_max) * k, f32::from(bb.y_max) * k],
        )
    }

    /// Normalises `text` to what can be drawn: NFC (so accents become single glyphs and
    /// need no mark positioning), whitespace as plain spaces, invisible controls
    /// removed, unsupported characters as [`REPLACEMENT`].
    pub fn sanitize(&self, text: &str) -> String {
        text.nfc()
            .filter(|c| !INVISIBLE.contains(c) && (!c.is_control() || c.is_whitespace()))
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .map(|c| if self.glyph(c).is_some() { c } else { REPLACEMENT })
            .collect()
    }

    /// Width in points of already-sanitised `text` at `size` pt.
    pub fn width(&self, text: &str, size: f32) -> f32 {
        text.chars().map(|c| self.advance_1000(c)).sum::<f32>() * size / 1000.0
    }
}

/// Number of leading characters of `text` that fit in `max_width` pt (at least one, so
/// callers always make progress).
fn prefix_len(face: &FontFace, text: &str, size: f32, max_width: f32) -> usize {
    let mut used = 0.0;
    let mut n = 0;
    for c in text.chars() {
        used += face.advance_1000(c) * size / 1000.0;
        if used > max_width {
            break;
        }
        n += 1;
    }
    n.max(1)
}

/// Fits sanitised `text` into `max_width` pt, ending it with an ellipsis when cut.
pub fn truncate(face: &FontFace, text: &str, size: f32, max_width: f32) -> String {
    if face.width(text, size) <= max_width {
        return text.to_owned();
    }
    const ELLIPSIS: char = '\u{2026}';
    let budget = max_width - face.width(&ELLIPSIS.to_string(), size);
    let n = prefix_len(face, text, size, budget);
    let mut out: String = text.chars().take(n).collect();
    out.push(ELLIPSIS);
    out
}

/// Greedy word wrap of sanitised `text` into lines no wider than `max_width` pt. Words
/// longer than a line are hard-cut so nothing overflows.
pub fn wrap(face: &FontFace, text: &str, size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ').filter(|w| !w.is_empty()) {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if face.width(&candidate, size) <= max_width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            let mut rest = word.to_owned();
            while face.width(&rest, size) > max_width {
                let n = prefix_len(face, &rest, size, max_width);
                lines.push(rest.chars().take(n).collect());
                rest = rest.chars().skip(n).collect();
            }
            line = rest;
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regular() -> FontFace {
        FontFace::load(Weight::Regular).unwrap()
    }

    #[test]
    fn sanitize_composes_accents_and_replaces_unsupported() {
        let f = regular();
        // "e" + combining acute becomes the single precomposed glyph.
        assert_eq!(f.sanitize("Cafe\u{301}"), "Caf\u{e9}");
        assert_eq!(f.sanitize("Z\u{fc}rich \u{41f}\u{440}\u{438}\u{432}\u{435}\u{442} \u{3b1}\u{3b2}"), "Z\u{fc}rich \u{41f}\u{440}\u{438}\u{432}\u{435}\u{442} \u{3b1}\u{3b2}");
        // CJK, emoji and Arabic are outside the subset: one replacement per character.
        assert_eq!(f.sanitize("\u{65e5}\u{672c}"), "\u{fffd}\u{fffd}");
        assert_eq!(f.sanitize("wifi\u{1f4f6}"), "wifi\u{fffd}");
        assert_eq!(f.sanitize("\u{627}\u{644}"), "\u{fffd}\u{fffd}");
    }

    #[test]
    fn sanitize_normalises_whitespace_and_drops_invisibles() {
        let f = regular();
        assert_eq!(f.sanitize("a\tb\nc\u{a0}d"), "a b c d");
        assert_eq!(f.sanitize("a\u{200b}b\u{202e}c\u{0}d"), "abcd");
    }

    #[test]
    fn characters_the_report_itself_prints_are_covered() {
        // The ellipsis is added by `truncate`; a missing glyph would silently become U+FFFD.
        let f = regular();
        for c in ['\u{2026}', '\u{fffd}', '-', '#', '(', ')', '/', '>', '<', ':', '%'] {
            assert!(f.glyph(c).is_some(), "{c:?} missing from the bundled font");
        }
        assert!(FontFace::load(Weight::Bold).unwrap().glyph('\u{2026}').is_some());
    }

    #[test]
    fn widths_are_positive_and_bold_is_wider() {
        let (r, b) = (regular(), FontFace::load(Weight::Bold).unwrap());
        let w = r.width("Access point", 10.0);
        assert!(w > 40.0 && w < 90.0, "implausible width {w}");
        assert!(b.width("Access point", 10.0) > w);
        assert!(r.width("iii", 10.0) < r.width("mmm", 10.0));
    }

    #[test]
    fn truncate_fits_and_marks_the_cut() {
        let f = regular();
        let t = truncate(&f, "A very long network name indeed", 10.0, 60.0);
        assert!(t.ends_with('\u{2026}'));
        assert!(f.width(&t, 10.0) <= 60.0);
        assert_eq!(truncate(&f, "short", 10.0, 60.0), "short");
    }

    #[test]
    fn wrap_respects_width_and_keeps_all_text() {
        let f = regular();
        let text = "one two three four five six seven eight nine ten";
        let lines = wrap(&f, text, 10.0, 80.0);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| f.width(l, 10.0) <= 80.0));
        assert_eq!(lines.join(" "), text);
        // An unbreakable word is hard-cut instead of overflowing.
        let long = wrap(&f, &"W".repeat(60), 10.0, 80.0);
        assert!(long.len() > 1 && long.iter().all(|l| f.width(l, 10.0) <= 80.0));
        assert_eq!(long.concat(), "W".repeat(60));
    }
}
