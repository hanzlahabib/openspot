//! Plan-space geometry for editing: distances, projections, snapping and hit testing.
//!
//! Everything here is in meters (the planning engine's unit) and independent of egui, so
//! the tool state machines and their tests need no display.

use openspot_model::{PlanningData, Point, Uuid};
use openspot_predict::geom::point_in_polygon;

/// Angle increment of the angle snap. 15 degrees contains the common 30/45/60/90 degree
/// corners and is fine enough to draw a deliberately skewed wall.
pub const ANGLE_STEP_DEG: f64 = 15.0;

/// Grid spacings offered for snapping, in meters.
pub const GRID_PRESETS_M: [f64; 4] = [0.1, 0.25, 0.5, 1.0];

/// Shorter walls are accidental clicks, not drawn walls.
pub const MIN_WALL_LENGTH_M: f64 = 0.05;

pub fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Where a point projects onto a segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentHit {
    /// Position along the segment, 0 at `a` and 1 at `b`.
    pub u: f64,
    pub point: Point,
    pub distance: f64,
}

/// Closest point of the segment `a -> b` to `p`. A zero-length segment is its start.
pub fn closest_on_segment(p: Point, a: Point, b: Point) -> SegmentHit {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let u = if len2 > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let point = Point::new(a.x + dx * u, a.y + dy * u);
    SegmentHit { u, point, distance: distance(p, point) }
}

/// What the snapping is allowed to do. Endpoint snapping wins over angle and grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapSettings {
    pub grid: bool,
    pub grid_m: f64,
    pub angle: bool,
    pub endpoints: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self { grid: true, grid_m: 0.25, angle: true, endpoints: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    None,
    Endpoint,
    Angle,
    Grid,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapped {
    pub point: Point,
    pub kind: SnapKind,
}

/// Every wall end and cutout vertex, except those of the wall `exclude` (the one being edited).
fn snap_targets(data: &PlanningData, exclude: Option<Uuid>) -> impl Iterator<Item = Point> {
    let walls = data.walls.iter().filter(move |w| Some(w.id) != exclude).flat_map(|w| [w.a, w.b]);
    let cutouts = data.cutouts.iter().flat_map(|c| c.polygon.iter().copied());
    walls.chain(cutouts)
}

fn round_to(v: f64, step: f64) -> f64 {
    if step > 0.0 { (v / step).round() * step } else { v }
}

/// Snaps `raw` for drawing. `anchor` is the fixed end of the segment being drawn (it enables
/// angle snapping); `tol_m` is the pick tolerance for endpoint snapping.
pub fn snap(raw: Point, anchor: Option<Point>, data: &PlanningData, s: &SnapSettings, tol_m: f64, exclude: Option<Uuid>) -> Snapped {
    if s.endpoints {
        let nearest = snap_targets(data, exclude).map(|t| (t, distance(raw, t))).filter(|(_, d)| *d <= tol_m).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((point, _)) = nearest {
            return Snapped { point, kind: SnapKind::Endpoint };
        }
    }
    if s.angle
        && let Some(a) = anchor
    {
        let (dx, dy) = (raw.x - a.x, raw.y - a.y);
        let length = dx.hypot(dy);
        if length > 0.0 {
            let step = ANGLE_STEP_DEG.to_radians();
            let angle = round_to(dy.atan2(dx), step);
            let length = if s.grid { round_to(length, s.grid_m) } else { length };
            return Snapped { point: Point::new(a.x + length * angle.cos(), a.y + length * angle.sin()), kind: SnapKind::Angle };
        }
    }
    if s.grid {
        return Snapped { point: Point::new(round_to(raw.x, s.grid_m), round_to(raw.y, s.grid_m)), kind: SnapKind::Grid };
    }
    Snapped { point: raw, kind: SnapKind::None }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    A,
    B,
}

/// Something under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Ap(Uuid),
    WallEnd { wall: Uuid, end: End },
    Opening { wall: Uuid, opening: Uuid, u: f64 },
    Wall { wall: Uuid, u: f64 },
    CutoutVertex { cutout: Uuid, index: usize },
    Cutout(Uuid),
}

/// The element at `p` within `tol_m`, front-most first: access points, wall ends, openings,
/// wall bodies, cutout vertices, cutout interiors. Walls are also hit within half their
/// drawn thickness so thick walls stay easy to pick when zoomed far in.
pub fn hit_test(data: &PlanningData, p: Point, tol_m: f64) -> Option<Hit> {
    let nearest = |items: Vec<(Hit, f64)>| items.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(h, _)| h);

    let aps = data.access_points.iter().map(|ap| (Hit::Ap(ap.id), distance(p, ap.position))).filter(|(_, d)| *d <= tol_m).collect();
    if let Some(hit) = nearest(aps) {
        return Some(hit);
    }
    let mut ends = Vec::new();
    for w in &data.walls {
        for (end, pt) in [(End::A, w.a), (End::B, w.b)] {
            let d = distance(p, pt);
            if d <= tol_m {
                ends.push((Hit::WallEnd { wall: w.id, end }, d));
            }
        }
    }
    if let Some(hit) = nearest(ends) {
        return Some(hit);
    }
    let mut bodies = Vec::new();
    for w in &data.walls {
        let seg = closest_on_segment(p, w.a, w.b);
        if seg.distance <= tol_m.max(w.thickness_m / 2.0) {
            let hit = match w.openings.iter().find(|o| o.contains(seg.u)) {
                Some(o) => Hit::Opening { wall: w.id, opening: o.id, u: seg.u },
                None => Hit::Wall { wall: w.id, u: seg.u },
            };
            bodies.push((hit, seg.distance));
        }
    }
    if let Some(hit) = nearest(bodies) {
        return Some(hit);
    }
    for c in &data.cutouts {
        let vertex = c.polygon.iter().enumerate().map(|(i, v)| (i, distance(p, *v))).filter(|(_, d)| *d <= tol_m).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((index, _)) = vertex {
            return Some(Hit::CutoutVertex { cutout: c.id, index });
        }
    }
    data.cutouts.iter().find(|c| point_in_polygon(p, &c.polygon)).map(|c| Hit::Cutout(c.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::{Cutout, Opening, OpeningKind, VirtualAp, Wall, WallKind};

    fn pt(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn wall(a: Point, b: Point) -> Wall {
        Wall::new(a, b, WallKind::Wall, "drywall")
    }

    #[test]
    fn closest_point_clamps_to_the_segment() {
        let h = closest_on_segment(pt(5.0, 3.0), pt(0.0, 0.0), pt(10.0, 0.0));
        assert_eq!((h.u, h.point, h.distance), (0.5, pt(5.0, 0.0), 3.0));
        let h = closest_on_segment(pt(-4.0, 3.0), pt(0.0, 0.0), pt(10.0, 0.0));
        assert_eq!((h.u, h.point, h.distance), (0.0, pt(0.0, 0.0), 5.0));
        let h = closest_on_segment(pt(13.0, 4.0), pt(0.0, 0.0), pt(10.0, 0.0));
        assert_eq!((h.u, h.distance), (1.0, 5.0));
        // Degenerate segment is a point.
        let h = closest_on_segment(pt(3.0, 4.0), pt(0.0, 0.0), pt(0.0, 0.0));
        assert_eq!((h.u, h.distance), (0.0, 5.0));
    }

    #[test]
    fn grid_snap_rounds_each_axis() {
        let s = SnapSettings { grid: true, grid_m: 0.5, angle: false, endpoints: false };
        let r = snap(pt(1.26, 2.74), None, &PlanningData::default(), &s, 0.1, None);
        assert_eq!((r.point, r.kind), (pt(1.5, 2.5), SnapKind::Grid));
        let off = SnapSettings { grid: false, ..s };
        let r = snap(pt(1.26, 2.74), None, &PlanningData::default(), &off, 0.1, None);
        assert_eq!((r.point, r.kind), (pt(1.26, 2.74), SnapKind::None));
    }

    #[test]
    fn angle_snap_locks_direction_and_rounds_length_to_the_grid() {
        let s = SnapSettings { grid: true, grid_m: 0.5, angle: true, endpoints: false };
        // About 3 degrees off horizontal, 4.1 m long: becomes exactly horizontal, 4.0 m.
        let raw = pt(1.0 + 4.1 * 3f64.to_radians().cos(), 1.0 + 4.1 * 3f64.to_radians().sin());
        let r = snap(raw, Some(pt(1.0, 1.0)), &PlanningData::default(), &s, 0.1, None);
        assert_eq!(r.kind, SnapKind::Angle);
        assert!((r.point.x - 5.0).abs() < 1e-9 && (r.point.y - 1.0).abs() < 1e-9, "{:?}", r.point);
        // Near 45 degrees stays on the diagonal.
        let r = snap(pt(3.1, 2.9), Some(pt(1.0, 1.0)), &PlanningData::default(), &s, 0.1, None);
        assert!((r.point.x - 1.0 - (r.point.y - 1.0)).abs() < 1e-9);
        // Without a grid the length is kept.
        let free = SnapSettings { grid: false, ..s };
        let r = snap(pt(4.0, 1.2), Some(pt(1.0, 1.0)), &PlanningData::default(), &free, 0.1, None);
        let want = (3.0f64).hypot(0.2);
        assert!((distance(r.point, pt(1.0, 1.0)) - want).abs() < 1e-9);
        assert!((r.point.y - 1.0).abs() < 1e-9, "15 degree step rounds 3.8 degrees to 0");
    }

    #[test]
    fn endpoints_win_and_the_edited_wall_is_excluded() {
        let mut data = PlanningData::default();
        let w = wall(pt(0.0, 0.0), pt(4.0, 0.0));
        let id = w.id;
        data.walls.push(w);
        data.cutouts.push(Cutout { id: Uuid::new_v4(), polygon: vec![pt(9.0, 9.0), pt(10.0, 9.0), pt(10.0, 10.0)] });
        let s = SnapSettings::default();
        let r = snap(pt(4.05, 0.07), Some(pt(0.0, 3.0)), &data, &s, 0.15, None);
        assert_eq!((r.point, r.kind), (pt(4.0, 0.0), SnapKind::Endpoint));
        let r = snap(pt(10.05, 9.95), None, &data, &s, 0.15, None);
        assert_eq!((r.point, r.kind), (pt(10.0, 10.0), SnapKind::Endpoint), "cutout vertices snap too");
        let r = snap(pt(4.05, 0.07), None, &data, &s, 0.15, Some(id));
        assert_eq!(r.kind, SnapKind::Grid, "a wall never snaps to itself");
        let r = snap(pt(4.5, 0.07), None, &data, &s, 0.15, None);
        assert_eq!(r.kind, SnapKind::Grid, "outside the tolerance");
    }

    #[test]
    fn hit_priorities_and_tolerance() {
        let mut data = PlanningData::default();
        let mut w = wall(pt(0.0, 0.0), pt(10.0, 0.0));
        let opening = Opening { id: Uuid::new_v4(), kind: OpeningKind::Door, start: 0.4, end: 0.5, material: "wood-door".into() };
        w.openings.push(opening.clone());
        let wid = w.id;
        data.walls.push(w);
        let ap = VirtualAp::new("a", pt(5.0, 0.0), vec![]);
        let apid = ap.id;

        assert_eq!(hit_test(&data, pt(2.0, 0.1), 0.2), Some(Hit::Wall { wall: wid, u: 0.2 }));
        assert_eq!(hit_test(&data, pt(2.0, 0.3), 0.2), None, "outside tolerance");
        assert_eq!(hit_test(&data, pt(0.05, 0.05), 0.2), Some(Hit::WallEnd { wall: wid, end: End::A }));
        assert!(matches!(hit_test(&data, pt(10.0, -0.1), 0.2), Some(Hit::WallEnd { end: End::B, .. })));
        match hit_test(&data, pt(4.4, 0.05), 0.2) {
            Some(Hit::Opening { wall, opening: o, .. }) => assert_eq!((wall, o), (wid, opening.id)),
            other => panic!("{other:?}"),
        }
        data.access_points.push(ap);
        assert_eq!(hit_test(&data, pt(5.05, 0.05), 0.2), Some(Hit::Ap(apid)), "AP is in front of the wall it hangs on");
        // A thick wall is hit within half its thickness even with a tiny tolerance.
        data.walls[0].thickness_m = 0.6;
        assert!(matches!(hit_test(&data, pt(2.0, 0.28), 0.01), Some(Hit::Wall { .. })));
    }

    #[test]
    fn cutouts_are_hit_by_vertex_then_interior() {
        let mut data = PlanningData::default();
        let c = Cutout { id: Uuid::new_v4(), polygon: vec![pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 4.0), pt(0.0, 4.0)] };
        let id = c.id;
        data.cutouts.push(c);
        assert_eq!(hit_test(&data, pt(4.05, 3.95), 0.2), Some(Hit::CutoutVertex { cutout: id, index: 2 }));
        assert_eq!(hit_test(&data, pt(2.0, 2.0), 0.2), Some(Hit::Cutout(id)));
        assert_eq!(hit_test(&data, pt(5.0, 2.0), 0.2), None);
    }
}
