//! Exact-enough 2-D segment geometry for wall crossings.
//!
//! Plan coordinates are meters, so an absolute tolerance of a nanometer is far below any
//! drawing resolution while still absorbing floating-point round-off.

use openspot_model::Point;

/// Absolute tolerance (m) for "on the line" and "at the end" tests.
const EPS: f64 = 1e-9;

/// Relative tolerance on the cross product of two directions for "parallel".
const PARALLEL_EPS: f64 = 1e-12;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intersection {
    None,
    /// Single common point at parameter `t` along the first segment and `u` along the second
    /// (both 0..=1, endpoints included).
    Point { t: f64, u: f64 },
    /// Collinear segments sharing a stretch of positive length: `t0 < t1` along the first
    /// segment, with the matching `u0`/`u1` along the second.
    Overlap { t0: f64, t1: f64, u0: f64, u1: f64 },
}

fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

/// Intersection of segment `p -> q` with segment `a -> b`, endpoints inclusive.
/// Zero-length segments never intersect anything.
pub fn intersect(p: Point, q: Point, a: Point, b: Point) -> Intersection {
    let r = sub(q, p);
    let s = sub(b, a);
    let (rr, ss) = (dot(r, r), dot(s, s));
    if rr == 0.0 || ss == 0.0 {
        return Intersection::None;
    }
    // Cheap reject on bounding boxes (hot path: every grid cell tests every wall).
    if p.x.max(q.x) < a.x.min(b.x) - EPS
        || p.x.min(q.x) > a.x.max(b.x) + EPS
        || p.y.max(q.y) < a.y.min(b.y) - EPS
        || p.y.min(q.y) > a.y.max(b.y) + EPS
    {
        return Intersection::None;
    }
    let ap = sub(a, p);
    let denom = cross(r, s);
    if denom.abs() <= PARALLEL_EPS * (rr * ss).sqrt() {
        // Parallel. Collinear only if `a` is on the path's line.
        if cross(ap, r).abs() / rr.sqrt() > EPS {
            return Intersection::None;
        }
        let ta = dot(ap, r) / rr;
        let tb = dot(sub(b, p), r) / rr;
        let (lo, hi) = (ta.min(tb), ta.max(tb));
        let (t0, t1) = (lo.max(0.0), hi.min(1.0));
        let tol = EPS / rr.sqrt();
        if t0 > t1 + tol {
            return Intersection::None;
        }
        // Wall parameter of path parameter t on the line: u = (t - ta) / (tb - ta).
        let u_at = |t: f64| ((t - ta) / (tb - ta)).clamp(0.0, 1.0);
        if t1 - t0 <= tol {
            let t = t0.clamp(0.0, 1.0);
            return Intersection::Point { t, u: u_at(t) };
        }
        let (u0, u1) = (u_at(t0), u_at(t1));
        return Intersection::Overlap { t0, t1, u0: u0.min(u1), u1: u0.max(u1) };
    }
    let t = cross(ap, s) / denom;
    let u = cross(ap, r) / denom;
    let (te, ue) = (EPS / rr.sqrt(), EPS / ss.sqrt());
    if t < -te || t > 1.0 + te || u < -ue || u > 1.0 + ue {
        return Intersection::None;
    }
    Intersection::Point { t: t.clamp(0.0, 1.0), u: u.clamp(0.0, 1.0) }
}

/// Tolerance on `t` (fraction of path) when deciding whether a crossing is at a path end.
const T_END_EPS: f64 = 1e-9;

/// Where a path crosses a wall for loss purposes, as a position `u` along the wall.
///
/// Rules (documented in `docs/PLANNING_ENGINE.md`):
/// * a path endpoint lying on a wall does not cross it (so loss is reciprocal and an AP
///   mounted on a wall is not penalised for it);
/// * a wall counts when the path touches or crosses its start (`u = 0`) but not when it
///   only touches its end (`u = 1`), so a polyline of connected walls is crossed once at a
///   shared vertex rather than twice;
/// * a path running along a wall counts that wall once.
pub fn wall_crossing(p: Point, q: Point, a: Point, b: Point) -> Option<f64> {
    match intersect(p, q, a, b) {
        Intersection::None => None,
        Intersection::Point { t, u } => {
            let interior = t > T_END_EPS && t < 1.0 - T_END_EPS;
            // Half-open in u. The tolerance mirrors `intersect`: u was clamped.
            let wall_len = ((b.x - a.x).hypot(b.y - a.y)).max(f64::MIN_POSITIVE);
            (interior && u < 1.0 - EPS / wall_len).then_some(u)
        }
        Intersection::Overlap { u0, u1, .. } => Some((u0 + u1) / 2.0),
    }
}

/// Even-odd point-in-polygon test; points on the boundary count as inside.
pub fn point_in_polygon(pt: Point, polygon: &[Point]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[j], polygon[i]);
        // On-edge test first so the boundary is deterministic.
        let ab = sub(b, a);
        let ap = sub(pt, a);
        let len2 = dot(ab, ab);
        if len2 > 0.0 {
            let along = dot(ap, ab) / len2;
            if (0.0..=1.0).contains(&along) && cross(ap, ab).abs() / len2.sqrt() <= EPS {
                return true;
            }
        }
        if (a.y > pt.y) != (b.y > pt.y) && pt.x < (b.x - a.x) * (pt.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn proper_crossing_reports_both_parameters() {
        let i = intersect(pt(0.0, 0.0), pt(10.0, 0.0), pt(4.0, -2.0), pt(4.0, 6.0));
        let Intersection::Point { t, u } = i else { panic!("{i:?}") };
        assert!((t - 0.4).abs() < 1e-12 && (u - 0.25).abs() < 1e-12);
    }

    #[test]
    fn disjoint_and_parallel_segments_do_not_intersect() {
        assert_eq!(intersect(pt(0.0, 0.0), pt(1.0, 0.0), pt(2.0, -1.0), pt(2.0, 1.0)), Intersection::None);
        assert_eq!(intersect(pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 1.0), pt(10.0, 1.0)), Intersection::None);
        // Supporting lines cross at (1.5, 1.5), beyond the end of the path.
        assert_eq!(intersect(pt(0.0, 0.0), pt(1.0, 1.0), pt(3.0, 0.0), pt(0.0, 3.0)), Intersection::None);
    }

    #[test]
    fn collinear_disjoint_touching_and_overlapping() {
        let (p, q) = (pt(0.0, 0.0), pt(10.0, 0.0));
        assert_eq!(intersect(p, q, pt(11.0, 0.0), pt(15.0, 0.0)), Intersection::None);
        // End-to-end touch at a single point.
        assert_eq!(intersect(p, q, pt(10.0, 0.0), pt(15.0, 0.0)), Intersection::Point { t: 1.0, u: 0.0 });
        // Wall drawn backwards from x=12 to x=7: shared stretch x=7..10, i.e. wall positions 0.4..1.0.
        let Intersection::Overlap { t0, t1, u0, u1 } = intersect(p, q, pt(12.0, 0.0), pt(7.0, 0.0)) else { panic!() };
        assert!((t0 - 0.7).abs() < 1e-12 && (t1 - 1.0).abs() < 1e-12);
        assert!((u0 - 0.4).abs() < 1e-12 && (u1 - 1.0).abs() < 1e-12);
    }

    #[test]
    fn t_junction_is_an_intersection_at_the_end() {
        // Wall ends exactly on the path (T): u = 1 for a wall drawn toward the path.
        assert_eq!(intersect(pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 3.0), pt(5.0, 0.0)), Intersection::Point { t: 0.5, u: 1.0 });
        // Path ends on the wall (T from the other side): t = 1.
        assert_eq!(intersect(pt(0.0, 0.0), pt(5.0, 0.0), pt(5.0, -3.0), pt(5.0, 3.0)), Intersection::Point { t: 1.0, u: 0.5 });
    }

    #[test]
    fn zero_length_segments_never_intersect() {
        assert_eq!(intersect(pt(1.0, 1.0), pt(1.0, 1.0), pt(0.0, 0.0), pt(2.0, 2.0)), Intersection::None);
        assert_eq!(intersect(pt(0.0, 0.0), pt(2.0, 2.0), pt(1.0, 1.0), pt(1.0, 1.0)), Intersection::None);
    }

    #[test]
    fn crossing_policy_endpoints_and_polylines() {
        let (p, q) = (pt(0.0, 0.0), pt(10.0, 0.0));
        // Ordinary crossing counts.
        assert_eq!(wall_crossing(p, q, pt(5.0, -1.0), pt(5.0, 1.0)), Some(0.5));
        // Path end on the wall: not crossed. Path start on the wall: not crossed.
        assert_eq!(wall_crossing(p, q, pt(10.0, -1.0), pt(10.0, 1.0)), None);
        assert_eq!(wall_crossing(p, q, pt(0.0, -1.0), pt(0.0, 1.0)), None);
        // Two connected walls meeting exactly on the path: counted once.
        let first = wall_crossing(p, q, pt(5.0, -2.0), pt(5.0, 0.0));
        let second = wall_crossing(p, q, pt(5.0, 0.0), pt(5.0, 2.0));
        assert_eq!(first.is_some() as u8 + second.is_some() as u8, 1);
        // Collinear overlap: once.
        assert!(wall_crossing(p, q, pt(2.0, 0.0), pt(6.0, 0.0)).is_some());
        // Collinear but disjoint: none.
        assert_eq!(wall_crossing(p, q, pt(12.0, 0.0), pt(16.0, 0.0)), None);
    }

    #[test]
    fn polygon_inside_outside_boundary_and_concave() {
        let sq = [pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 4.0), pt(0.0, 4.0)];
        assert!(point_in_polygon(pt(2.0, 2.0), &sq));
        assert!(!point_in_polygon(pt(5.0, 2.0), &sq));
        assert!(point_in_polygon(pt(4.0, 2.0), &sq));
        assert!(point_in_polygon(pt(0.0, 0.0), &sq));
        // L-shape: the notch is outside.
        let l = [pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 2.0), pt(2.0, 2.0), pt(2.0, 4.0), pt(0.0, 4.0)];
        assert!(point_in_polygon(pt(1.0, 3.0), &l));
        assert!(!point_in_polygon(pt(3.0, 3.0), &l));
        assert!(!point_in_polygon(pt(1.0, 1.0), &[pt(0.0, 0.0), pt(1.0, 1.0)]));
    }
}
