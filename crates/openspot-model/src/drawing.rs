//! Vector floor plans drawn inside the app, for sites with no plan to import.
//!
//! The shapes stay editable data; the app renders them to the zone's map image.

use crate::Uuid;

pub type Pt = (f64, f64);

/// Stroke width presets, in map pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineThickness {
    Thin,
    #[default]
    Medium,
    Thick,
}

impl LineThickness {
    pub const ALL: [Self; 3] = [Self::Thin, Self::Medium, Self::Thick];

    pub fn label(self) -> &'static str {
        match self {
            Self::Thin => "Thin",
            Self::Medium => "Medium",
            Self::Thick => "Thick",
        }
    }

    /// Stroke width in map pixels: 2 px stays visible on a 1600 px plan, 8 px reads as a
    /// load-bearing wall.
    pub fn px(self) -> f64 {
        match self {
            Self::Thin => 2.0,
            Self::Medium => 4.0,
            Self::Thick => 8.0,
        }
    }

    pub fn code(self) -> i64 {
        Self::ALL.iter().position(|t| *t == self).map_or(0, |i| i as i64)
    }

    pub fn from_code(code: i64) -> Option<Self> {
        usize::try_from(code).ok().and_then(|i| Self::ALL.get(i).copied())
    }
}

/// Label size presets, in map pixels of font height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontSize {
    Small,
    #[default]
    Medium,
    Large,
    Huge,
}

impl FontSize {
    pub const ALL: [Self; 4] = [Self::Small, Self::Medium, Self::Large, Self::Huge];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::Huge => "Huge",
        }
    }

    /// Font height in map pixels: 16 px is legible on a 1600 px plan when fitted to a
    /// window; the larger steps label rooms and the whole floor.
    pub fn px(self) -> f32 {
        match self {
            Self::Small => 16.0,
            Self::Medium => 24.0,
            Self::Large => 36.0,
            Self::Huge => 56.0,
        }
    }

    pub fn code(self) -> i64 {
        Self::ALL.iter().position(|t| *t == self).map_or(1, |i| i as i64)
    }

    pub fn from_code(code: i64) -> Option<Self> {
        usize::try_from(code).ok().and_then(|i| Self::ALL.get(i).copied())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShapeKind {
    /// Open line through two or more points.
    Polyline { points: Vec<Pt> },
    /// `filled` paints the whole rectangle (pillars, furniture); otherwise only the outline.
    Rect { min: Pt, max: Pt, filled: bool },
    /// Text with its top-left corner at `at`; `\n` starts a new line.
    Text { at: Pt, text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub id: Uuid,
    pub kind: ShapeKind,
    /// Stroke width for lines and rectangle outlines (unused by text).
    pub thickness: LineThickness,
    /// Text height (unused by lines and rectangles).
    pub font: FontSize,
}

impl Shape {
    pub fn new(kind: ShapeKind, thickness: LineThickness, font: FontSize) -> Self {
        Self { id: Uuid::new_v4(), kind, thickness, font }
    }

    /// The shape moved by `(dx, dy)`.
    pub fn translated(&self, dx: f64, dy: f64) -> Self {
        let mv = |p: Pt| (p.0 + dx, p.1 + dy);
        let kind = match &self.kind {
            ShapeKind::Polyline { points } => ShapeKind::Polyline { points: points.iter().copied().map(mv).collect() },
            ShapeKind::Rect { min, max, filled } => ShapeKind::Rect { min: mv(*min), max: mv(*max), filled: *filled },
            ShapeKind::Text { at, text } => ShapeKind::Text { at: mv(*at), text: text.clone() },
        };
        Self { kind, ..self.clone() }
    }
}

/// The drawn plan of a zone: canvas size in map pixels plus shapes, bottom to top.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub width: u32,
    pub height: u32,
    pub shapes: Vec<Shape>,
}

impl Drawing {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, shapes: Vec::new() }
    }

    pub fn remove(&mut self, id: Uuid) -> bool {
        let before = self.shapes.len();
        self.shapes.retain(|s| s.id != id);
        self.shapes.len() != before
    }

    pub fn get(&self, id: Uuid) -> Option<&Shape> {
        self.shapes.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: Uuid) -> Option<&mut Shape> {
        self.shapes.iter_mut().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_moves_every_point_and_keeps_style_and_id() {
        let s = Shape::new(ShapeKind::Polyline { points: vec![(0.0, 0.0), (10.0, 5.0)] }, LineThickness::Thick, FontSize::Small);
        let t = s.translated(3.0, -2.0);
        assert_eq!(t.kind, ShapeKind::Polyline { points: vec![(3.0, -2.0), (13.0, 3.0)] });
        assert_eq!((t.id, t.thickness), (s.id, LineThickness::Thick));
        let r = Shape::new(ShapeKind::Rect { min: (1.0, 1.0), max: (2.0, 2.0), filled: true }, LineThickness::Thin, FontSize::Medium).translated(1.0, 1.0);
        assert_eq!(r.kind, ShapeKind::Rect { min: (2.0, 2.0), max: (3.0, 3.0), filled: true });
        let x = Shape::new(ShapeKind::Text { at: (5.0, 5.0), text: "a".into() }, LineThickness::Thin, FontSize::Medium).translated(-5.0, 0.0);
        assert_eq!(x.kind, ShapeKind::Text { at: (0.0, 5.0), text: "a".into() });
    }

    #[test]
    fn remove_deletes_only_the_given_shape() {
        let mut d = Drawing::new(100, 100);
        let a = Shape::new(ShapeKind::Rect { min: (0.0, 0.0), max: (1.0, 1.0), filled: false }, LineThickness::Thin, FontSize::Medium);
        let b = a.clone();
        let b = Shape { id: Uuid::new_v4(), ..b };
        let (ida, idb) = (a.id, b.id);
        d.shapes.extend([a, b]);
        assert!(d.remove(ida));
        assert!(!d.remove(ida));
        assert_eq!(d.shapes.len(), 1);
        assert!(d.get(idb).is_some());
    }

    #[test]
    fn preset_codes_round_trip_and_sizes_grow() {
        for t in LineThickness::ALL {
            assert_eq!(LineThickness::from_code(t.code()), Some(t));
        }
        for f in FontSize::ALL {
            assert_eq!(FontSize::from_code(f.code()), Some(f));
        }
        assert_eq!(LineThickness::from_code(7), None);
        assert!(LineThickness::ALL.windows(2).all(|w| w[0].px() < w[1].px()));
        assert!(FontSize::ALL.windows(2).all(|w| w[0].px() < w[1].px()));
    }
}
