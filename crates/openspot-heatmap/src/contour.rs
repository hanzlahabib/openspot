//! Vector isolines: marching triangles over the Delaunay triangulation of the samples.
//!
//! Inside the sample hull this follows exactly the surface the raster heatmap draws (linear
//! interpolation per triangle), but as polylines in map pixels that stay sharp at any
//! zoom and export to vector formats. Nothing is drawn beyond the hull.

use std::collections::HashMap;

use delaunator::{Point, triangulate};

use crate::Sample;
use crate::grid::merge_duplicates;

/// A connected contour line in map pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    pub points: Vec<(f64, f64)>,
    /// First and last point coincide (the line is a loop). Open lines end on the hull boundary.
    pub closed: bool,
}

/// All contour lines at one value.
#[derive(Debug, Clone, PartialEq)]
pub struct Isoline {
    pub level: f32,
    pub lines: Vec<Polyline>,
}

/// `count` evenly spaced levels strictly inside `lo..hi` (the end values are the ramp's
/// saturated colors, so contours there carry no information).
pub fn evenly_spaced_levels(lo: f32, hi: f32, count: usize) -> Vec<f32> {
    if count == 0 || !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Vec::new();
    }
    (1..=count).map(|i| lo + (hi - lo) * i as f32 / (count + 1) as f32).collect()
}

/// An undirected triangulation edge, identified by its two sample indices (low first).
type EdgeKey = (usize, usize);

fn edge_key(a: usize, b: usize) -> EdgeKey {
    (a.min(b), a.max(b))
}

/// Contours of the piecewise-linear surface through `samples` at each of `levels`.
/// A sample exactly at a level counts as "at or above", so a contour never splits in two
/// at a vertex. Levels that are not finite are skipped; fewer than three distinct samples
/// give no contours.
pub fn isolines(samples: &[Sample], levels: &[f32]) -> Vec<Isoline> {
    let samples = merge_duplicates(samples);
    if samples.len() < 3 {
        return Vec::new();
    }
    let points: Vec<Point> = samples.iter().map(|s| Point { x: s.x, y: s.y }).collect();
    let triangles = triangulate(&points).triangles;
    levels
        .iter()
        .filter(|l| l.is_finite())
        .map(|&level| Isoline { level, lines: contour_at(&samples, &triangles, f64::from(level)) })
        .collect()
}

fn contour_at(samples: &[Sample], triangles: &[usize], level: f64) -> Vec<Polyline> {
    // Crossing point on an edge, by linear interpolation of the endpoint values.
    let crossing = |(a, b): EdgeKey| {
        let (pa, pb) = (samples[a], samples[b]);
        let t = (level - pa.value) / (pb.value - pa.value);
        (pa.x + t * (pb.x - pa.x), pa.y + t * (pb.y - pa.y))
    };
    let above = |i: usize| samples[i].value >= level;

    // One segment per triangle that the level passes through, between its two crossed edges.
    let mut segments: Vec<(EdgeKey, EdgeKey)> = Vec::new();
    for t in triangles.chunks_exact(3) {
        let crossed: Vec<EdgeKey> = [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
            .into_iter()
            .filter(|&(a, b)| above(a) != above(b))
            .map(|(a, b)| edge_key(a, b))
            .collect();
        // A triangle's vertices split 3-0 or 2-1 by the above/below test, so there are 0 or 2.
        if let [e0, e1] = crossed[..] {
            segments.push((e0, e1));
        }
    }

    // Segments meeting at a shared edge continue each other: chain them via that edge.
    let mut at_edge: HashMap<EdgeKey, Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segments.iter().enumerate() {
        at_edge.entry(*a).or_default().push(i);
        at_edge.entry(*b).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    let mut lines = Vec::new();

    // Open lines first, starting from their free ends on the hull boundary; whatever is
    // left afterwards can only be loops.
    let mut starts: Vec<usize> = (0..segments.len())
        .filter(|&i| [segments[i].0, segments[i].1].iter().any(|e| at_edge[e].len() == 1))
        .collect();
    starts.extend(0..segments.len());
    for start in starts {
        if used[start] {
            continue;
        }
        let (a, b) = segments[start];
        // Begin at the free end if there is one so open lines are walked end to end.
        let first = if at_edge[&a].len() == 1 { a } else if at_edge[&b].len() == 1 { b } else { a };
        let other = if first == a { b } else { a };
        used[start] = true;
        let mut chain = vec![first, other];
        let mut tip = other;
        while let Some(&next) = at_edge[&tip].iter().find(|&&i| !used[i]) {
            used[next] = true;
            let (x, y) = segments[next];
            tip = if x == tip { y } else { x };
            chain.push(tip);
        }
        let closed = chain.len() > 2 && chain.first() == chain.last();
        lines.push(Polyline { points: chain.into_iter().map(crossing).collect(), closed });
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples on a regular grid over `[x0, x1] x [y0, y1]` with `step` spacing.
    fn grid_samples(x0: f64, x1: f64, y0: f64, y1: f64, step: f64, f: impl Fn(f64, f64) -> f64) -> Vec<Sample> {
        let mut out = Vec::new();
        let (nx, ny) = (((x1 - x0) / step).round() as usize, ((y1 - y0) / step).round() as usize);
        for j in 0..=ny {
            for i in 0..=nx {
                let (x, y) = (x0 + i as f64 * step, y0 + j as f64 * step);
                out.push(Sample { x, y, value: f(x, y) });
            }
        }
        out
    }

    fn orient(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    }

    /// Proper crossing of two segments (touching endpoints do not count).
    fn cross(p: ((f64, f64), (f64, f64)), q: ((f64, f64), (f64, f64))) -> bool {
        let (d1, d2) = (orient(q.0, q.1, p.0), orient(q.0, q.1, p.1));
        let (d3, d4) = (orient(p.0, p.1, q.0), orient(p.0, p.1, q.1));
        d1 * d2 < -1e-9 && d3 * d4 < -1e-9
    }

    fn segments(lines: &[Polyline]) -> Vec<((f64, f64), (f64, f64))> {
        lines.iter().flat_map(|l| l.points.windows(2).map(|w| (w[0], w[1]))).collect()
    }

    #[test]
    fn planar_field_gives_one_straight_line_at_the_expected_x() {
        let samples = grid_samples(0.0, 100.0, 0.0, 100.0, 10.0, |x, _| x);
        let out = isolines(&samples, &[35.0]);
        assert_eq!(out.len(), 1);
        let lines = &out[0].lines;
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(!lines[0].closed);
        assert!(lines[0].points.iter().all(|p| (p.0 - 35.0).abs() < 1e-9), "{:?}", lines[0].points);
        let ys: Vec<f64> = lines[0].points.iter().map(|p| p.1).collect();
        assert!((ys.iter().cloned().fold(f64::MAX, f64::min) - 0.0).abs() < 1e-9);
        assert!((ys.iter().cloned().fold(f64::MIN, f64::max) - 100.0).abs() < 1e-9);
        // Walked end to end: y is monotonic.
        assert!(ys.windows(2).all(|w| w[0] <= w[1]) || ys.windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn tilted_plane_line_has_the_right_slope_and_position() {
        // value = x + 2y; the level 100 line is x + 2y = 100.
        let samples = grid_samples(0.0, 100.0, 0.0, 100.0, 12.5, |x, y| x + 2.0 * y);
        let out = isolines(&samples, &[100.0]);
        assert_eq!(out[0].lines.len(), 1);
        for p in &out[0].lines[0].points {
            assert!((p.0 + 2.0 * p.1 - 100.0).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn radial_field_gives_a_near_circular_closed_contour() {
        let samples = grid_samples(-100.0, 100.0, -100.0, 100.0, 5.0, |x, y| x.hypot(y));
        let out = isolines(&samples, &[50.0]);
        let lines = &out[0].lines;
        assert_eq!(lines.len(), 1, "one loop");
        let line = &lines[0];
        assert!(line.closed);
        for p in &line.points {
            assert!((p.0.hypot(p.1) - 50.0).abs() < 0.5, "{p:?} radius {}", p.0.hypot(p.1));
        }
        // Shoelace area of the loop against pi r^2.
        let area = line.points.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum::<f64>().abs() / 2.0;
        let expected = std::f64::consts::PI * 2500.0;
        assert!((area - expected).abs() / expected < 0.02, "area {area} vs {expected}");
    }

    #[test]
    fn levels_never_cross_each_other() {
        let samples = grid_samples(-100.0, 100.0, -100.0, 100.0, 5.0, |x, y| x.hypot(y));
        let levels = evenly_spaced_levels(0.0, 100.0, 4);
        let out = isolines(&samples, &levels);
        assert_eq!(out.len(), 4);
        // Concentric: each level sits at its own radius.
        for iso in &out {
            assert!(!iso.lines.is_empty());
            for p in iso.lines.iter().flat_map(|l| &l.points) {
                assert!((p.0.hypot(p.1) - f64::from(iso.level)).abs() < 0.5);
            }
        }
        for (i, a) in out.iter().enumerate() {
            for b in &out[i + 1..] {
                for sa in segments(&a.lines) {
                    for sb in segments(&b.lines) {
                        assert!(!cross(sa, sb), "levels {} and {} cross", a.level, b.level);
                    }
                }
            }
        }
        // Sanity: the crossing test itself detects a crossing.
        assert!(cross(((0.0, 0.0), (2.0, 2.0)), ((0.0, 2.0), (2.0, 0.0))));
    }

    #[test]
    fn a_level_equal_to_sample_values_still_yields_one_connected_line() {
        let samples = grid_samples(0.0, 100.0, 0.0, 100.0, 10.0, |x, _| x);
        let out = isolines(&samples, &[30.0]);
        assert_eq!(out[0].lines.len(), 1, "{:?}", out[0].lines);
        assert!(out[0].lines[0].points.iter().all(|p| (p.0 - 30.0).abs() < 1e-9));
    }

    #[test]
    fn levels_outside_the_data_and_degenerate_input_give_nothing() {
        let samples = grid_samples(0.0, 100.0, 0.0, 100.0, 10.0, |x, _| x);
        let out = isolines(&samples, &[-5.0, 500.0, f32::NAN]);
        assert_eq!(out.len(), 2, "NaN level skipped");
        assert!(out.iter().all(|i| i.lines.is_empty()));
        assert!(isolines(&samples[..2], &[10.0]).is_empty());
        assert!(isolines(&[], &[10.0]).is_empty());
        // Collinear points have no area, hence no contour.
        let line: Vec<Sample> = (0..10).map(|i| Sample { x: f64::from(i), y: 0.0, value: f64::from(i) }).collect();
        assert!(isolines(&line, &[4.5]).iter().all(|i| i.lines.is_empty()));
    }

    #[test]
    fn duplicate_sample_positions_are_averaged_not_fatal() {
        let mut samples = grid_samples(0.0, 50.0, 0.0, 50.0, 10.0, |x, _| x);
        samples.push(Sample { x: 0.0, y: 0.0, value: 0.0 });
        let out = isolines(&samples, &[25.0]);
        assert_eq!(out[0].lines.len(), 1);
    }

    #[test]
    fn evenly_spaced_levels_are_interior() {
        assert_eq!(evenly_spaced_levels(0.0, 100.0, 4), vec![20.0, 40.0, 60.0, 80.0]);
        assert!(evenly_spaced_levels(0.0, 100.0, 0).is_empty());
        assert!(evenly_spaced_levels(5.0, 5.0, 3).is_empty());
        assert!(evenly_spaced_levels(f32::NAN, 5.0, 3).is_empty());
    }
}
