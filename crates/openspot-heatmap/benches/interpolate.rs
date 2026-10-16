//! Interpolation / rasterisation cost on a realistic large survey (Q-04).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use openspot_heatmap::{GridSpec, Metric, Sample, Smoothing, interpolate, nearest, rasterize, smooth};
use openspot_model::{ScanPoint, Snapshot};
use openspot_wlan::{AccessPoint, MacAddress};

/// Survey size from the acceptance criterion.
const POINTS: usize = 10_000;
/// A 2000 x 1500 px floor plan at 4 px per cell = 500 x 375 cells.
const SPEC: GridSpec = GridSpec { width_px: 2000.0, height_px: 1500.0, cell_px: 4.0, extend_px: 0.0 };

/// Deterministic scatter (xorshift) so runs are comparable without a rand dependency.
fn scatter(n: usize) -> Vec<Sample> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| {
            let (x, y) = (next() * SPEC.width_px, next() * SPEC.height_px);
            // A smooth field plus noise, in dBm.
            Sample { x, y, value: -40.0 - 0.02 * x - 0.01 * y + next() * 4.0 }
        })
        .collect()
}

fn bench_interpolate(c: &mut Criterion) {
    let samples = scatter(POINTS);
    let mut group = c.benchmark_group("heatmap");
    group.sample_size(10);
    group.bench_function("interpolate_10k_triangulated", |b| {
        b.iter(|| interpolate(black_box(&samples), black_box(&SPEC)));
    });
    // IDW extension is O(cells x samples); it is what edge extrapolation costs.
    let extended = GridSpec { extend_px: 60.0, ..SPEC };
    group.bench_function("interpolate_10k_with_edge_extension", |b| {
        b.iter(|| interpolate(black_box(&samples), black_box(&extended)));
    });
    group.bench_function("nearest_10k_category", |b| {
        b.iter(|| nearest(black_box(&samples), black_box(&SPEC), 40.0));
    });
    let grid = interpolate(&samples, &SPEC);
    group.bench_function("smooth_strong", |b| {
        b.iter(|| smooth(black_box(&grid), Smoothing::Strong.radius_cells()));
    });
    // Full path used by the app: sampling a snapshot, interpolation and smoothing.
    let snapshot = snapshot_of(&samples);
    let metric = Metric::Signal(Default::default());
    group.bench_function("rasterize_10k_signal", |b| {
        b.iter(|| rasterize(black_box(&snapshot), black_box(&metric), black_box(&SPEC), Smoothing::Normal));
    });
    group.finish();
}

fn snapshot_of(samples: &[Sample]) -> Snapshot {
    let mut snapshot = Snapshot::new("bench");
    snapshot.points = samples
        .iter()
        .map(|s| {
            let ap = |last: u8, offset: f64| {
                // Truncation to whole dBm is what scans report.
                let rssi = (s.value + offset) as i32;
                AccessPoint::from_beacon(String::new(), MacAddress([2, 0, 0, 0, 0, last]), rssi, 5_180_000, true, 100, vec![])
            };
            ScanPoint::new(s.x, s.y, 0, vec![ap(1, 0.0), ap(2, -9.0), ap(3, -17.0)])
        })
        .collect();
    snapshot
}

criterion_group!(benches, bench_interpolate);
criterion_main!(benches);
