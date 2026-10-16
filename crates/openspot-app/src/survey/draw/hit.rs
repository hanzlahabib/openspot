//! Hit-testing and bounds of shapes, in map pixels.

use openspot_model::{Drawing, Pt, Shape, ShapeKind, Uuid};

use super::text::text_size;

/// Distance from `p` to the segment `a`-`b`.
pub(super) fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// Axis-aligned bounds `(min, max)` of the shape including its stroke or text extent.
pub fn shape_bounds(shape: &Shape) -> Option<(Pt, Pt)> {
    let half = shape.thickness.px() / 2.0;
    match &shape.kind {
        ShapeKind::Polyline { points } => {
            let xs = points.iter().map(|p| p.0);
            let ys = points.iter().map(|p| p.1);
            let min = (xs.clone().fold(f64::INFINITY, f64::min), ys.clone().fold(f64::INFINITY, f64::min));
            let max = (xs.fold(f64::NEG_INFINITY, f64::max), ys.fold(f64::NEG_INFINITY, f64::max));
            (!points.is_empty()).then_some(((min.0 - half, min.1 - half), (max.0 + half, max.1 + half)))
        }
        ShapeKind::Rect { min, max, .. } => Some(((min.0.min(max.0) - half, min.1.min(max.1) - half), (min.0.max(max.0) + half, min.1.max(max.1) + half))),
        ShapeKind::Text { at, text } => {
            let (w, h) = text_size(text, shape.font.px())?;
            Some((*at, (at.0 + w, at.1 + h)))
        }
    }
}

fn in_box(p: Pt, (min, max): (Pt, Pt), margin: f64) -> bool {
    p.0 >= min.0 - margin && p.0 <= max.0 + margin && p.1 >= min.1 - margin && p.1 <= max.1 + margin
}

/// Whether `p` is on the shape, with `tolerance` extra pixels of slack (a finger-width
/// target when zoomed out). Outline rectangles are hit on their edge, not their inside.
pub fn hit(shape: &Shape, p: Pt, tolerance: f64) -> bool {
    let reach = tolerance + shape.thickness.px() / 2.0;
    match &shape.kind {
        ShapeKind::Polyline { points } => match points.as_slice() {
            [] => false,
            [only] => (p.0 - only.0).hypot(p.1 - only.1) <= reach,
            pts => pts.windows(2).any(|w| distance_to_segment(p, w[0], w[1]) <= reach),
        },
        ShapeKind::Rect { min, max, filled } => {
            let (lo, hi) = ((min.0.min(max.0), min.1.min(max.1)), (min.0.max(max.0), min.1.max(max.1)));
            if *filled {
                return in_box(p, (lo, hi), reach);
            }
            let corners = [(lo.0, lo.1), (hi.0, lo.1), (hi.0, hi.1), (lo.0, hi.1)];
            (0..4).any(|i| distance_to_segment(p, corners[i], corners[(i + 1) % 4]) <= reach)
        }
        ShapeKind::Text { .. } => shape_bounds(shape).is_some_and(|b| in_box(p, b, tolerance)),
    }
}

/// The topmost shape at `p`.
pub fn top_hit(d: &Drawing, p: Pt, tolerance: f64) -> Option<Uuid> {
    d.shapes.iter().rev().find(|s| hit(s, p, tolerance)).map(|s| s.id)
}
