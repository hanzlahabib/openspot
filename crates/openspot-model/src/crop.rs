//! The part of a floor plan where the heatmap is drawn and exported.

/// Region in map pixels. Outside it the heatmap is not drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum CropArea {
    Rect { min: (f64, f64), max: (f64, f64) },
    /// At least three vertices; edges close automatically.
    Polygon(Vec<(f64, f64)>),
}

impl CropArea {
    /// Rectangle from any two opposite corners.
    pub fn rect(a: (f64, f64), b: (f64, f64)) -> Self {
        Self::Rect { min: (a.0.min(b.0), a.1.min(b.1)), max: (a.0.max(b.0), a.1.max(b.1)) }
    }

    /// `None` unless there are three or more vertices.
    pub fn polygon(points: Vec<(f64, f64)>) -> Option<Self> {
        (points.len() >= 3).then_some(Self::Polygon(points))
    }

    /// Whether `(x, y)` is inside. Rectangle edges count as inside; polygon uses the
    /// even-odd rule (a ray cast to +x), so self-intersections leave holes.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        match self {
            Self::Rect { min, max } => (min.0..=max.0).contains(&x) && (min.1..=max.1).contains(&y),
            Self::Polygon(pts) => {
                let mut inside = false;
                let mut prev = pts[pts.len() - 1];
                for &cur in pts {
                    // Edge straddles the ray's y and the crossing is right of the point.
                    if (cur.1 > y) != (prev.1 > y) && x < (prev.0 - cur.0) * (y - cur.1) / (prev.1 - cur.1) + cur.0 {
                        inside = !inside;
                    }
                    prev = cur;
                }
                inside
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_normalises_corners_and_includes_edges() {
        let r = CropArea::rect((10.0, 20.0), (0.0, 5.0));
        assert_eq!(r, CropArea::Rect { min: (0.0, 5.0), max: (10.0, 20.0) });
        assert!(r.contains(0.0, 5.0) && r.contains(10.0, 20.0) && r.contains(5.0, 10.0));
        assert!(!r.contains(-0.1, 10.0) && !r.contains(5.0, 20.1));
    }

    #[test]
    fn triangle_containment() {
        let t = CropArea::polygon(vec![(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]).unwrap();
        assert!(t.contains(2.0, 2.0));
        assert!(t.contains(1.0, 8.0));
        assert!(!t.contains(8.0, 8.0), "beyond the hypotenuse");
        assert!(!t.contains(-1.0, 1.0));
        assert!(!t.contains(11.0, 1.0));
    }

    #[test]
    fn concave_polygon_excludes_the_notch() {
        // A "U": the notch between x 4..6 above y 5 is outside.
        let u = CropArea::polygon(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (6.0, 10.0), (6.0, 5.0), (4.0, 5.0), (4.0, 10.0), (0.0, 10.0)]).unwrap();
        assert!(u.contains(2.0, 8.0) && u.contains(8.0, 8.0) && u.contains(5.0, 2.0));
        assert!(!u.contains(5.0, 8.0));
    }

    #[test]
    fn polygon_needs_three_vertices_and_vertex_order_is_irrelevant() {
        assert!(CropArea::polygon(vec![(0.0, 0.0), (1.0, 1.0)]).is_none());
        let cw = CropArea::polygon(vec![(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]).unwrap();
        let ccw = CropArea::polygon(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]).unwrap();
        for (x, y) in [(5.0, 5.0), (11.0, 5.0), (5.0, -1.0)] {
            assert_eq!(cw.contains(x, y), ccw.contains(x, y));
        }
    }
}
