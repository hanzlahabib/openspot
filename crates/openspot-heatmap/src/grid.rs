use delaunator::{Point, triangulate};

use crate::job::Control;
use crate::nearest_index::SampleIndex;

/// A measured value at a map position (pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub x: f64,
    pub y: f64,
    pub value: f64,
}

/// Map area to cover, and raster resolution.
#[derive(Debug, Clone, Copy)]
pub struct GridSpec {
    pub width_px: f64,
    pub height_px: f64,
    /// Edge length of one grid cell in map pixels.
    pub cell_px: f64,
    /// How far beyond the sample hull values are extrapolated (0 disables).
    pub extend_px: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub cell_px: f64,
    /// Row-major, `None` where there is no data.
    pub values: Vec<Option<f32>>,
}

impl Grid {
    pub fn get(&self, col: usize, row: usize) -> Option<f32> {
        self.values.get(row * self.cols + col).copied().flatten()
    }
}

/// How strongly a continuous heatmap is blurred to hide interpolation seams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Smoothing {
    Off,
    #[default]
    Normal,
    Strong,
    Extra,
}

impl Smoothing {
    pub const ALL: [Self; 4] = [Self::Off, Self::Normal, Self::Strong, Self::Extra];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Normal => "Normal",
            Self::Strong => "Strong",
            Self::Extra => "Extra strong",
        }
    }

    /// Blur radius in grid cells. The grid has ~160 cells on its long side, so these are
    /// roughly 0.6 %, 1.2 % and 2.5 % of the map: noticeable but never washing out walls.
    pub fn radius_cells(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Normal => 1,
            Self::Strong => 2,
            Self::Extra => 4,
        }
    }
}

/// Box blur that averages only over cells holding data, so empty areas stay empty and
/// edges do not fade toward zero.
pub fn smooth(grid: &Grid, radius_cells: usize) -> Grid {
    smooth_with(grid, radius_cells, &Control::NONE)
}

/// [`smooth`] that can be cancelled and reports progress; a cancelled result is partial
/// and must be discarded by the caller (check [`Control::is_cancelled`]).
pub fn smooth_with(grid: &Grid, radius_cells: usize, ctl: &Control) -> Grid {
    if radius_cells == 0 {
        return grid.clone();
    }
    let r = radius_cells as isize;
    let mut out = grid.clone();
    for row in 0..grid.rows {
        if ctl.is_cancelled() {
            return out;
        }
        ctl.report(row as f32 / grid.rows as f32);
        for col in 0..grid.cols {
            if grid.get(col, row).is_none() {
                continue;
            }
            let (mut sum, mut n) = (0.0f32, 0u32);
            for dy in -r..=r {
                for dx in -r..=r {
                    let (c, rr) = (col as isize + dx, row as isize + dy);
                    if c < 0 || rr < 0 || c >= grid.cols as isize || rr >= grid.rows as isize {
                        continue;
                    }
                    if let Some(v) = grid.get(c as usize, rr as usize) {
                        sum += v;
                        n += 1;
                    }
                }
            }
            out.values[row * grid.cols + col] = Some(sum / n as f32);
        }
    }
    out
}

/// Fills each cell with the category of the nearest sample within `radius_px` (empty
/// beyond it). Categories cannot be averaged, so there is no triangulation here.
pub fn nearest(samples: &[Sample], spec: &GridSpec, radius_px: f64) -> Grid {
    nearest_with(samples, spec, radius_px, &Control::NONE)
}

/// [`nearest`] that can be cancelled; a cancelled result is partial and must be discarded.
pub fn nearest_with(samples: &[Sample], spec: &GridSpec, radius_px: f64, ctl: &Control) -> Grid {
    let cols = ((spec.width_px / spec.cell_px).ceil() as usize).max(1);
    let rows = ((spec.height_px / spec.cell_px).ceil() as usize).max(1);
    let r2 = radius_px * radius_px;
    let index = SampleIndex::new(samples, radius_px);
    let mut values = vec![None; cols * rows];
    for row in 0..rows {
        if ctl.is_cancelled() {
            break;
        }
        ctl.report(row as f32 / rows as f32);
        for col in 0..cols {
            let (px, py) = ((col as f64 + 0.5) * spec.cell_px, (row as f64 + 0.5) * spec.cell_px);
            let best = match &index {
                Some(index) => index.nearest_value(px, py, r2),
                None => nearest_value_scan(samples, px, py, r2),
            };
            values[row * cols + col] = best.map(|v| v as f32);
        }
    }
    Grid { cols, rows, cell_px: spec.cell_px, values }
}

/// Value of the sample nearest to `(px, py)` within squared distance `r2`, by scanning every
/// sample; the first of equally near samples wins. This is the reference the spatial index
/// must reproduce exactly, and the fallback when the radius is not finite.
fn nearest_value_scan(samples: &[Sample], px: f64, py: f64, r2: f64) -> Option<f64> {
    samples
        .iter()
        .map(|s| ((s.x - px).powi(2) + (s.y - py).powi(2), s.value))
        .filter(|(d2, _)| *d2 <= r2)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, v)| v)
}

/// Samples closer than this (in pixels) are the same spot; their values are averaged.
const MERGE_EPSILON_PX: f64 = 1e-6;
/// Triangles filled between cancellation checks: frequent enough to react within a few
/// milliseconds, rare enough that the atomic load is not measurable.
const TRIANGLES_PER_CHECK: usize = 64;
/// Tolerance when testing whether a cell center lies inside a triangle.
const INSIDE_EPSILON: f64 = 1e-9;

pub fn interpolate(samples: &[Sample], spec: &GridSpec) -> Grid {
    interpolate_with(samples, spec, &Control::NONE)
}

/// [`interpolate`] that can be cancelled and reports progress; a cancelled result is
/// partial and must be discarded by the caller.
pub fn interpolate_with(samples: &[Sample], spec: &GridSpec, ctl: &Control) -> Grid {
    let cols = ((spec.width_px / spec.cell_px).ceil() as usize).max(1);
    let rows = ((spec.height_px / spec.cell_px).ceil() as usize).max(1);
    let mut values = vec![None; cols * rows];
    let samples = merge_duplicates(samples);

    if samples.len() >= 3 {
        fill_triangles(&samples, spec.cell_px, cols, rows, &mut values, &ctl.scoped(0.0, 0.5));
    }
    if spec.extend_px > 0.0 || samples.len() < 3 {
        // With too few points for a triangulation, IDW is all there is, so give it a
        // sensible radius even if the caller asked for none.
        let radius = if spec.extend_px > 0.0 { spec.extend_px } else { spec.cell_px * 10.0 };
        fill_idw(&samples, spec.cell_px, cols, rows, radius, &mut values, &ctl.scoped(0.5, 1.0));
    }
    Grid { cols, rows, cell_px: spec.cell_px, values }
}

pub(crate) fn merge_duplicates(samples: &[Sample]) -> Vec<Sample> {
    let mut out: Vec<(Sample, u32)> = Vec::with_capacity(samples.len());
    for s in samples.iter().filter(|s| s.x.is_finite() && s.y.is_finite() && s.value.is_finite()) {
        match out.iter_mut().find(|(o, _)| (o.x - s.x).abs() < MERGE_EPSILON_PX && (o.y - s.y).abs() < MERGE_EPSILON_PX) {
            Some((o, n)) => {
                o.value = (o.value * f64::from(*n) + s.value) / f64::from(*n + 1);
                *n += 1;
            }
            None => out.push((*s, 1)),
        }
    }
    out.into_iter().map(|(s, _)| s).collect()
}

fn fill_triangles(samples: &[Sample], cell: f64, cols: usize, rows: usize, values: &mut [Option<f32>], ctl: &Control) {
    let points: Vec<Point> = samples.iter().map(|s| Point { x: s.x, y: s.y }).collect();
    let tri = triangulate(&points);
    let total = tri.triangles.len() / 3;
    for (n, t) in tri.triangles.chunks_exact(3).enumerate() {
        if n % TRIANGLES_PER_CHECK == 0 {
            if ctl.is_cancelled() {
                return;
            }
            ctl.report(n as f32 / total as f32);
        }
        let (a, b, c) = (samples[t[0]], samples[t[1]], samples[t[2]]);
        let det = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
        if det.abs() < f64::EPSILON {
            continue;
        }
        let min_x = a.x.min(b.x).min(c.x);
        let max_x = a.x.max(b.x).max(c.x);
        let min_y = a.y.min(b.y).min(c.y);
        let max_y = a.y.max(b.y).max(c.y);
        let col_range = cell_range(min_x, max_x, cell, cols);
        let row_range = cell_range(min_y, max_y, cell, rows);
        for row in row_range {
            for col in col_range.clone() {
                let (px, py) = ((col as f64 + 0.5) * cell, (row as f64 + 0.5) * cell);
                let l1 = ((b.y - c.y) * (px - c.x) + (c.x - b.x) * (py - c.y)) / det;
                let l2 = ((c.y - a.y) * (px - c.x) + (a.x - c.x) * (py - c.y)) / det;
                let l3 = 1.0 - l1 - l2;
                if l1 >= -INSIDE_EPSILON && l2 >= -INSIDE_EPSILON && l3 >= -INSIDE_EPSILON {
                    values[row * cols + col] = Some((l1 * a.value + l2 * b.value + l3 * c.value) as f32);
                }
            }
        }
    }
}

/// Cells whose centers could lie in `[lo, hi]`, clamped to the grid.
fn cell_range(lo: f64, hi: f64, cell: f64, count: usize) -> std::ops::Range<usize> {
    let first = ((lo / cell) - 0.5).floor().max(0.0) as usize;
    let last = (((hi / cell) - 0.5).ceil().max(0.0) as usize + 1).min(count);
    first.min(count)..last
}

fn fill_idw(samples: &[Sample], cell: f64, cols: usize, rows: usize, radius: f64, values: &mut [Option<f32>], ctl: &Control) {
    let r2 = radius * radius;
    for row in 0..rows {
        if ctl.is_cancelled() {
            return;
        }
        ctl.report(row as f32 / rows as f32);
        for col in 0..cols {
            let slot = &mut values[row * cols + col];
            if slot.is_some() {
                continue;
            }
            let (px, py) = ((col as f64 + 0.5) * cell, (row as f64 + 0.5) * cell);
            let (mut wsum, mut vsum) = (0.0, 0.0);
            for s in samples {
                let d2 = (s.x - px).powi(2) + (s.y - py).powi(2);
                if d2 > r2 {
                    continue;
                }
                let w = 1.0 / d2.max(1e-12);
                wsum += w;
                vsum += w * s.value;
            }
            if wsum > 0.0 {
                *slot = Some((vsum / wsum) as f32);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    /// Deterministic generator so failures reproduce; no rand dependency.
    struct Xorshift(u64);

    impl Xorshift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// Uniform in `0..n`.
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn unit(&mut self) -> f64 {
            (self.next() >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// The pre-index implementation, kept verbatim as the oracle.
    fn nearest_brute_force(samples: &[Sample], spec: &GridSpec, radius_px: f64) -> Grid {
        let cols = ((spec.width_px / spec.cell_px).ceil() as usize).max(1);
        let rows = ((spec.height_px / spec.cell_px).ceil() as usize).max(1);
        let r2 = radius_px * radius_px;
        let mut values = vec![None; cols * rows];
        for row in 0..rows {
            for col in 0..cols {
                let (px, py) = ((col as f64 + 0.5) * spec.cell_px, (row as f64 + 0.5) * spec.cell_px);
                let best = samples
                    .iter()
                    .map(|s| ((s.x - px).powi(2) + (s.y - py).powi(2), s.value))
                    .filter(|(d2, _)| *d2 <= r2)
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                values[row * cols + col] = best.map(|(_, v)| v as f32);
            }
        }
        Grid { cols, rows, cell_px: spec.cell_px, values }
    }

    fn same_bits(a: &Grid, b: &Grid) -> bool {
        a.cols == b.cols && a.rows == b.rows && a.values.iter().map(|v| v.map(f32::to_bits)).eq(b.values.iter().map(|v| v.map(f32::to_bits)))
    }

    #[test]
    fn spatial_index_gives_exactly_the_brute_force_result_on_random_inputs() {
        let mut rng = Xorshift(0xDEAD_BEEF_CAFE_F00D);
        for case in 0..200 {
            let spec = GridSpec {
                width_px: 20.0 + rng.unit() * 300.0,
                height_px: 20.0 + rng.unit() * 300.0,
                cell_px: [2.5, 4.0, 7.0][rng.below(3) as usize],
                extend_px: 0.0,
            };
            let count = rng.below(120) as usize;
            // Mix of free positions and integer lattice positions: the lattice produces many
            // exactly equal distances, which is where tie-breaking could differ.
            let samples: Vec<Sample> = (0..count)
                .map(|_| {
                    let lattice = rng.below(2) == 0;
                    let (x, y) = if lattice {
                        (rng.below(40) as f64 * 5.0 - 20.0, rng.below(40) as f64 * 5.0 - 20.0)
                    } else {
                        (rng.unit() * spec.width_px * 1.4 - 0.2 * spec.width_px, rng.unit() * spec.height_px * 1.4 - 0.2 * spec.height_px)
                    };
                    // Few distinct values so equal-distance samples often disagree.
                    Sample { x, y, value: rng.below(5) as f64 }
                })
                .collect();
            let radius = [0.0, 0.5, 3.0, 12.5, 40.0, 120.0, 1.0e6][rng.below(7) as usize];
            let fast = nearest(&samples, &spec, radius);
            let slow = nearest_brute_force(&samples, &spec, radius);
            assert!(same_bits(&fast, &slow), "case {case}: {count} samples, radius {radius}, spec {spec:?}");
        }
    }

    #[test]
    fn equal_distances_resolve_to_the_earlier_sample_like_the_scan() {
        let spec = GridSpec { width_px: 10.0, height_px: 10.0, cell_px: 10.0, extend_px: 0.0 };
        // Cell centre (5, 5); both samples are exactly 3 px away.
        let a = Sample { x: 2.0, y: 5.0, value: 1.0 };
        let b = Sample { x: 8.0, y: 5.0, value: 2.0 };
        assert_eq!(nearest(&[a, b], &spec, 10.0).values, vec![Some(1.0)]);
        assert_eq!(nearest(&[b, a], &spec, 10.0).values, vec![Some(2.0)]);
    }

    #[test]
    fn the_radius_bound_is_inclusive_and_nothing_beyond_it_matches() {
        let spec = GridSpec { width_px: 10.0, height_px: 10.0, cell_px: 10.0, extend_px: 0.0 };
        let s = [Sample { x: 5.0, y: 0.0, value: 7.0 }]; // exactly 5 px from the centre
        assert_eq!(nearest(&s, &spec, 5.0).values, vec![Some(7.0)]);
        assert_eq!(nearest(&s, &spec, 4.999).values, vec![None]);
    }

    #[test]
    fn hostile_samples_and_radii_behave_like_the_scan() {
        let spec = GridSpec { width_px: 30.0, height_px: 30.0, cell_px: 3.0, extend_px: 0.0 };
        let samples = [
            Sample { x: f64::NAN, y: 5.0, value: 1.0 },
            Sample { x: f64::INFINITY, y: 5.0, value: 2.0 },
            Sample { x: 1.0e300, y: -1.0e300, value: 3.0 },
            Sample { x: 10.0, y: 10.0, value: 4.0 },
            Sample { x: 10.0, y: 10.0, value: f64::NAN },
        ];
        for radius in [0.0, 4.0, 1.0e300, f64::INFINITY, f64::NAN, -3.0] {
            let fast = nearest(&samples, &spec, radius);
            let slow = nearest_brute_force(&samples, &spec, radius);
            assert!(same_bits(&fast, &slow), "radius {radius}");
        }
    }

    #[test]
    fn empty_input_and_cancellation_still_work() {
        let spec = GridSpec { width_px: 20.0, height_px: 20.0, cell_px: 4.0, extend_px: 0.0 };
        assert!(nearest(&[], &spec, 10.0).values.iter().all(Option::is_none));
        let ctl = Control::NONE;
        assert_eq!(nearest_with(&[], &spec, 10.0, &ctl).cols, 5);
    }
    use super::*;

    fn plane(x: f64, y: f64) -> f64 {
        10.0 + x + 2.0 * y
    }

    fn square_samples() -> Vec<Sample> {
        [(0.0, 0.0), (100.0, 0.0), (0.0, 100.0), (100.0, 100.0)]
            .into_iter()
            .map(|(x, y)| Sample { x, y, value: plane(x, y) })
            .collect()
    }

    #[test]
    fn linear_field_is_reproduced_inside_hull() {
        let spec = GridSpec { width_px: 100.0, height_px: 100.0, cell_px: 10.0, extend_px: 0.0 };
        let g = interpolate(&square_samples(), &spec);
        assert_eq!((g.cols, g.rows), (10, 10));
        for row in 0..g.rows {
            for col in 0..g.cols {
                let (x, y) = ((col as f64 + 0.5) * 10.0, (row as f64 + 0.5) * 10.0);
                let v = g.get(col, row).expect("inside hull") as f64;
                assert!((v - plane(x, y)).abs() < 1e-3, "({col},{row}): {v} vs {}", plane(x, y));
            }
        }
    }

    #[test]
    fn outside_hull_is_empty_without_extension_and_filled_with_it() {
        let samples = vec![
            Sample { x: 20.0, y: 20.0, value: -50.0 },
            Sample { x: 40.0, y: 20.0, value: -60.0 },
            Sample { x: 30.0, y: 40.0, value: -70.0 },
        ];
        let mut spec = GridSpec { width_px: 100.0, height_px: 100.0, cell_px: 10.0, extend_px: 0.0 };
        let g = interpolate(&samples, &spec);
        assert!(g.get(9, 9).is_none());
        spec.extend_px = 200.0;
        let g = interpolate(&samples, &spec);
        assert!(g.get(9, 9).is_some());
    }

    #[test]
    fn fewer_than_three_samples_fall_back_to_idw() {
        let spec = GridSpec { width_px: 50.0, height_px: 50.0, cell_px: 10.0, extend_px: 0.0 };
        let g = interpolate(&[Sample { x: 25.0, y: 25.0, value: -55.0 }], &spec);
        assert!(g.values.iter().flatten().any(|&v| (v + 55.0).abs() < 1e-3));
        assert!(interpolate(&[], &spec).values.iter().all(Option::is_none));
    }

    #[test]
    fn collinear_samples_do_not_panic() {
        let line: Vec<Sample> = (0..5).map(|i| Sample { x: f64::from(i) * 10.0, y: 10.0, value: f64::from(i) }).collect();
        let spec = GridSpec { width_px: 50.0, height_px: 50.0, cell_px: 5.0, extend_px: 30.0 };
        let g = interpolate(&line, &spec);
        assert!(g.values.iter().any(Option::is_some));
    }

    #[test]
    fn smoothing_preserves_constant_fields_and_holes_and_softens_spikes() {
        let mut values = vec![Some(-60.0f32); 25];
        values[12] = Some(-20.0); // spike in the middle of a 5x5 grid
        values[0] = None;
        let grid = Grid { cols: 5, rows: 5, cell_px: 1.0, values };
        let out = smooth(&grid, 1);
        assert_eq!(out.get(0, 0), None);
        let spike = out.get(2, 2).unwrap();
        assert!(spike < -20.0 && spike > -60.0, "{spike}");
        let flat = Grid { cols: 3, rows: 3, cell_px: 1.0, values: vec![Some(-50.0); 9] };
        assert!(smooth(&flat, 2).values.iter().flatten().all(|v| (*v + 50.0).abs() < 1e-4));
        assert_eq!(smooth(&grid, 0), grid);
    }

    #[test]
    fn smoothing_levels_grow() {
        let r: Vec<_> = Smoothing::ALL.iter().map(|s| s.radius_cells()).collect();
        assert!(r.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn nearest_fills_voronoi_cells_within_radius() {
        let samples = [Sample { x: 10.0, y: 50.0, value: 1.0 }, Sample { x: 90.0, y: 50.0, value: 2.0 }];
        let spec = GridSpec { width_px: 100.0, height_px: 100.0, cell_px: 10.0, extend_px: 0.0 };
        let g = nearest(&samples, &spec, 30.0);
        assert_eq!(g.get(0, 5), Some(1.0));
        assert_eq!(g.get(9, 5), Some(2.0));
        assert_eq!(g.get(5, 5), None); // 40 px from either sample
        assert_eq!(g.get(0, 0), None); // too far vertically
    }

    #[test]
    fn duplicate_positions_are_averaged() {
        let merged = merge_duplicates(&[
            Sample { x: 1.0, y: 1.0, value: -40.0 },
            Sample { x: 1.0, y: 1.0, value: -60.0 },
            Sample { x: 2.0, y: 2.0, value: -10.0 },
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].value, -50.0);
    }

    #[test]
    fn non_finite_samples_are_ignored() {
        let merged = merge_duplicates(&[Sample { x: f64::NAN, y: 0.0, value: 1.0 }, Sample { x: 0.0, y: 0.0, value: f64::INFINITY }]);
        assert!(merged.is_empty());
    }
}
