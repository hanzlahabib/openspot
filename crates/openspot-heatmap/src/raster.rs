//! Turning a metric plus survey data into a classified or interpolated grid, and pixels.

use openspot_model::Snapshot;

use crate::grid::{interpolate_with, nearest_with, smooth_with};
use crate::job::{Cancelled, Control};
use crate::{Gradient, GradientPreset, Grid, GridSpec, Metric, Sample, Smoothing, samples};

/// How far categorical maps reach beyond sample points, relative to the IDW extension
/// used by continuous maps: flat colors are less misleading than numbers far from data,
/// so they are allowed a little more reach to avoid a speckled look.
const CATEGORY_REACH_FACTOR: f64 = 2.0;
/// Share of a continuous rasterization's progress bar given to interpolation; smoothing is
/// a single cheap pass over the result.
const INTERPOLATE_SHARE: f32 = 0.85;

/// Rasterizes `metric` for `snapshot` (smoothed linear interpolation for continuous
/// metrics, nearest sample for categorical ones, which are never blurred). `None` when there is nothing to show.
pub fn rasterize(snapshot: &Snapshot, metric: &Metric, spec: &GridSpec, smoothing: Smoothing) -> Option<Grid> {
    // `NONE` never cancels, so the error arm is unreachable; `ok()` keeps the signature simple.
    rasterize_samples(&samples(snapshot, metric), metric, spec, smoothing, &Control::NONE).ok().flatten()
}

/// [`rasterize`] from already extracted `samples` (see [`samples`]), cancellable and with
/// progress. Splitting the cheap sample extraction from the expensive interpolation lets a
/// background job own just the samples instead of a copy of the whole snapshot. Returns
/// `Ok(None)` when there is nothing to show and `Err(Cancelled)` if `ctl` was cancelled.
pub fn rasterize_samples(samples: &[Sample], metric: &Metric, spec: &GridSpec, smoothing: Smoothing, ctl: &Control) -> Result<Option<Grid>, Cancelled> {
    if let Metric::Troubleshoot { base, acceptable, critical } = metric {
        let Some(grid) = rasterize_samples(samples, base, spec, smoothing, ctl)? else { return Ok(None) };
        // A categorical base is already classified; thresholds make no sense for it.
        return Ok(Some(if base.palette().is_some() { grid } else { classify(grid, *acceptable, *critical) }));
    }
    if samples.is_empty() {
        return Ok(None);
    }
    let grid = if metric.palette().is_some() {
        nearest_with(samples, spec, spec.extend_px * CATEGORY_REACH_FACTOR, &ctl.scoped(0.0, 1.0))
    } else {
        let raw = interpolate_with(samples, spec, &ctl.scoped(0.0, INTERPOLATE_SHARE));
        ctl.check()?;
        smooth_with(&raw, smoothing.radius_cells(), &ctl.scoped(INTERPOLATE_SHARE, 1.0))
    };
    ctl.check()?;
    Ok(Some(grid))
}

/// RGBA pixels for a grid produced by [`rasterize`] with the same `metric`, using each
/// metric's own colors.
pub fn colorize(metric: &Metric, grid: &Grid, alpha: u8) -> Vec<u8> {
    colorize_with(metric, grid, alpha, GradientPreset::Default)
}

/// Like [`colorize`] with a color scheme for continuous metrics. Categorical palettes are
/// fixed so their legend always matches the map.
pub fn colorize_with(metric: &Metric, grid: &Grid, alpha: u8, preset: GradientPreset) -> Vec<u8> {
    colorize_ranged(metric, grid, alpha, preset, None)
}

/// The gradient a continuous `metric` is drawn with: its own ramp, optionally stretched
/// over a user `range` (H-15), then recolored by `preset`. Legends use this too so the
/// bar always matches the map.
pub fn effective_gradient(metric: &Metric, preset: GradientPreset, range: Option<(f32, f32)>) -> Gradient {
    let base = metric.gradient();
    let base = match range {
        Some((lo, hi)) => base.rescale(lo, hi),
        None => base,
    };
    base.with_preset(preset)
}

/// [`colorize_with`] with an optional user value range for continuous metrics (ignored for
/// categorical ones, which have no scale).
pub fn colorize_ranged(metric: &Metric, grid: &Grid, alpha: u8, preset: GradientPreset, range: Option<(f32, f32)>) -> Vec<u8> {
    match metric.palette() {
        Some(p) => p.render_rgba(grid, alpha),
        None => effective_gradient(metric, preset, range).render_rgba(grid, alpha),
    }
}

/// Stable key for per-metric settings such as the value range: the metric's variant
/// name, so all signal-level variants (best, selected, one AP) share one dBm range.
pub fn metric_key(metric: &Metric) -> String {
    let debug = format!("{metric:?}");
    debug.chars().take_while(|c| c.is_alphanumeric()).collect()
}


/// Maps interpolated values to 0 = Critical, 1 = Acceptable, 2 = Good.
fn classify(mut grid: Grid, acceptable: i32, critical: i32) -> Grid {
    let (acceptable, critical) = (acceptable as f32, critical as f32);
    for v in grid.values.iter_mut().flatten() {
        *v = if *v >= acceptable {
            2.0
        } else if *v <= critical {
            0.0
        } else {
            1.0
        };
    }
    grid
}

/// Fraction of mapped cells in each of `categories` classes (all zero for an empty grid).
pub fn category_shares(grid: &Grid, categories: usize) -> Vec<f64> {
    let mut counts = vec![0usize; categories];
    for v in grid.values.iter().flatten() {
        counts[(v.round().max(0.0) as usize).min(categories - 1)] += 1;
    }
    let total: usize = counts.iter().sum();
    counts.into_iter().map(|c| if total == 0 { 0.0 } else { c as f64 / total as f64 }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::ScanPoint;
    use openspot_wlan::{AccessPoint, MacAddress};

    fn ap(rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, 1]), rssi, 2_437_000, true, 100, vec![])
    }

    fn line_snapshot() -> Snapshot {
        let mut s = Snapshot::new("s");
        // Signal falls from -50 dBm on the left edge to -90 dBm on the right edge.
        for (x, y, rssi) in [(0.0, 0.0, -50), (100.0, 0.0, -90), (0.0, 100.0, -50), (100.0, 100.0, -90)] {
            s.points.push(ScanPoint::new(x, y, 0, vec![ap(rssi)]));
        }
        s
    }

    fn spec() -> GridSpec {
        GridSpec { width_px: 100.0, height_px: 100.0, cell_px: 10.0, extend_px: 40.0 }
    }

    #[test]
    fn rasterize_and_colorize_handle_both_kinds() {
        let spec = spec();
        let snap = line_snapshot();
        let cont = Metric::Signal(vec![]);
        let g = rasterize(&snap, &cont, &spec, Smoothing::Off).unwrap();
        assert_eq!(colorize(&cont, &g, 200).len(), g.cols * g.rows * 4);
        let cat = Metric::BandCoverage { wanted: vec![], min_rssi_dbm: -80 };
        let g = rasterize(&snap, &cat, &spec, Smoothing::Strong).unwrap();
        assert!(g.values.iter().flatten().all(|v| v.fract() == 0.0));
        assert!(rasterize(&Snapshot::new("empty"), &cont, &spec, Smoothing::Off).is_none());
    }


    #[test]
    fn tx_rate_map_interpolates_recorded_links_and_is_empty_without_them() {
        let mut snap = line_snapshot();
        assert!(rasterize(&snap, &Metric::TxRate, &spec(), Smoothing::Off).is_none(), "no link recorded, nothing to show");
        // Rate falls from 800 Mbps on the left edge to 200 Mbps on the right edge.
        for p in &mut snap.points {
            let rate = if p.x < 50.0 { 800.0 } else { 200.0 };
            p.link = Some(openspot_model::LinkInfo { bssid: openspot_wlan::MacAddress([2; 6]), tx_rate_mbps: rate, rx_rate_mbps: rate });
        }
        let g = rasterize(&snap, &Metric::TxRate, &spec(), Smoothing::Off).unwrap();
        let (lo, hi) = g.values.iter().flatten().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(lo >= 200.0 && hi <= 800.0, "linear interpolation never overshoots the recorded rates: {lo}..{hi}");
        let left = g.get(0, 5).unwrap();
        let right = g.get(g.cols - 1, 5).unwrap();
        // Cell centres are 5 px from each edge, so 5 % of the way along the 800 -> 200 ramp.
        assert!((left - 770.0).abs() < 1.0 && (right - 230.0).abs() < 1.0, "{left} / {right}");
        assert_eq!(colorize(&Metric::TxRate, &g, 255).len(), g.cols * g.rows * 4);
    }

    #[test]
    fn troubleshoot_classifies_by_threshold() {
        let snap = line_snapshot();
        let metric = Metric::Troubleshoot { base: Box::new(Metric::Signal(vec![])), acceptable: -67, critical: -75 };
        let grid = rasterize(&snap, &metric, &spec(), Smoothing::Off).unwrap();
        let classes: std::collections::BTreeSet<i32> = grid.values.iter().flatten().map(|v| *v as i32).collect();
        assert!(classes.is_subset(&[0, 1, 2].into()));
        assert!(classes.contains(&2) || classes.contains(&0), "{classes:?}");
        let shares = category_shares(&grid, 3);
        assert!((shares.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert_eq!(metric.palette().unwrap().entries().len(), 3);
        assert_eq!(metric.unit(), "dBm");
    }

    #[test]
    fn classify_boundaries_are_inclusive_toward_the_worse_class_only_for_critical() {
        let grid = Grid { cols: 4, rows: 1, cell_px: 1.0, values: vec![Some(-60.0), Some(-67.0), Some(-70.0), Some(-75.0)] };
        let c = classify(grid, -67, -75);
        assert_eq!(c.values, vec![Some(2.0), Some(2.0), Some(1.0), Some(0.0)]);
    }

    #[test]
    fn preset_changes_continuous_pixels_but_not_categorical_ones() {
        let snap = line_snapshot();
        let cont = Metric::Signal(vec![]);
        let g = rasterize(&snap, &cont, &spec(), Smoothing::Off).unwrap();
        assert_ne!(colorize_with(&cont, &g, 255, GradientPreset::BlueRed), colorize(&cont, &g, 255));
        let cat = Metric::BandCoverage { wanted: vec![], min_rssi_dbm: -80 };
        let g = rasterize(&snap, &cat, &spec(), Smoothing::Off).unwrap();
        assert_eq!(colorize_with(&cat, &g, 255, GradientPreset::BlueRed), colorize(&cat, &g, 255));
    }

    #[test]
    fn controlled_path_equals_the_synchronous_one_for_every_kind_of_metric() {
        let snap = line_snapshot();
        let metrics = [
            Metric::Signal(vec![]),
            Metric::ApCount { min_rssi_dbm: -80 },
            Metric::BandCoverage { wanted: vec![], min_rssi_dbm: -80 },
            Metric::Troubleshoot { base: Box::new(Metric::Signal(vec![])), acceptable: -67, critical: -75 },
        ];
        let (cancel, progress) = (crate::job::CancelToken::default(), crate::job::Progress::default());
        for metric in metrics {
            for smoothing in Smoothing::ALL {
                let sync = rasterize(&snap, &metric, &spec(), smoothing);
                let ctl = Control::new(&cancel, &progress);
                let via_ctl = rasterize_samples(&samples(&snap, &metric), &metric, &spec(), smoothing, &ctl).unwrap();
                assert_eq!(sync, via_ctl, "{metric:?} {smoothing:?}");
            }
        }
        assert!(progress.get() > 0.0, "progress was reported");
    }

    fn wait_for<T>(job: &mut crate::job::Job<T>) -> crate::job::JobPoll<T> {
        use crate::job::JobPoll;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            match job.poll() {
                JobPoll::Pending if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(2)),
                other => return other,
            }
        }
    }

    #[test]
    fn background_job_result_equals_the_synchronous_result() {
        let snap = line_snapshot();
        let metric = Metric::Signal(vec![]);
        let expected = rasterize(&snap, &metric, &spec(), Smoothing::Strong);
        let (s, m) = (samples(&snap, &metric), metric.clone());
        let mut job = crate::job::Job::spawn("test", move |ctl| rasterize_samples(&s, &m, &spec(), Smoothing::Strong, ctl)).unwrap();
        match wait_for(&mut job) {
            crate::job::JobPoll::Done(grid) => assert_eq!(grid, expected),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(job.progress(), 1.0);
    }

    #[test]
    fn a_heavy_rasterization_stops_soon_after_cancel() {
        // 9 million cells with IDW extension: seconds of work if not cancelled.
        let big = GridSpec { width_px: 3000.0, height_px: 3000.0, cell_px: 1.0, extend_px: 400.0 };
        let metric = Metric::Signal(vec![]);
        let s = samples(&line_snapshot(), &metric);
        let mut job = crate::job::Job::spawn("test", move |ctl| rasterize_samples(&s, &metric, &big, Smoothing::Extra, ctl)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(job.poll(), crate::job::JobPoll::Pending, "still working");
        let start = std::time::Instant::now();
        job.cancel();
        assert_eq!(wait_for(&mut job), crate::job::JobPoll::Cancelled);
        // Generous: on an idle machine this takes ~0.3 s, but shared CI runners (or a loaded
        // dev box) can be 10x slower. Without cancellation the job runs far longer than this.
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "took {:?}", start.elapsed());
    }

    #[test]
    fn a_cancelled_control_aborts_rasterization() {
        let snap = line_snapshot();
        let metric = Metric::Signal(vec![]);
        let (cancel, progress) = (crate::job::CancelToken::default(), crate::job::Progress::default());
        cancel.cancel();
        let ctl = Control::new(&cancel, &progress);
        assert_eq!(rasterize_samples(&samples(&snap, &metric), &metric, &spec(), Smoothing::Normal, &ctl), Err(Cancelled));
    }

    #[test]
    fn user_range_restretches_the_gradient_only_for_continuous_metrics() {
        let snap = line_snapshot();
        let cont = Metric::Signal(vec![]);
        let g = rasterize(&snap, &cont, &spec(), Smoothing::Off).unwrap();
        let default = colorize_with(&cont, &g, 255, GradientPreset::Default);
        let narrow = colorize_ranged(&cont, &g, 255, GradientPreset::Default, Some((-95.0, -85.0)));
        assert_ne!(default, narrow);
        assert_eq!(colorize_ranged(&cont, &g, 255, GradientPreset::Default, None), default);
        // Values here run -90..-50: with the range ending at -85 almost everything saturates top color.
        let top = Gradient::signal_dbm().color(-35.0);
        assert!(narrow.chunks(4).filter(|p| p[3] == 255).filter(|p| p[..3] == top).count() > 0);
        let cat = Metric::BandCoverage { wanted: vec![], min_rssi_dbm: -80 };
        let g = rasterize(&snap, &cat, &spec(), Smoothing::Off).unwrap();
        assert_eq!(colorize_ranged(&cat, &g, 255, GradientPreset::Default, Some((0.0, 1.0))), colorize(&cat, &g, 255));
    }

    #[test]
    fn metric_key_is_the_variant_name() {
        assert_eq!(metric_key(&Metric::Signal(vec![])), "Signal");
        assert_eq!(metric_key(&Metric::Signal(vec![openspot_wlan::MacAddress([1; 6])])), "Signal");
        assert_eq!(metric_key(&Metric::ApCount { min_rssi_dbm: -80 }), "ApCount");
        assert_eq!(metric_key(&Metric::Sir(vec![])), "Sir");
    }

    #[test]
    fn smoothing_changes_continuous_but_not_categorical_grids() {
        let snap = line_snapshot();
        let cont = Metric::Signal(vec![]);
        let off = rasterize(&snap, &cont, &spec(), Smoothing::Off).unwrap();
        let extra = rasterize(&snap, &cont, &spec(), Smoothing::Extra).unwrap();
        assert_ne!(off, extra);
        let cat = Metric::BandCoverage { wanted: vec![], min_rssi_dbm: -80 };
        assert_eq!(
            rasterize(&snap, &cat, &spec(), Smoothing::Off).unwrap(),
            rasterize(&snap, &cat, &spec(), Smoothing::Extra).unwrap()
        );
    }

    #[test]
    fn shares_of_empty_grid_are_zero() {
        let grid = Grid { cols: 1, rows: 1, cell_px: 1.0, values: vec![None] };
        assert_eq!(category_shares(&grid, 3), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn default_thresholds_exist_only_for_continuous_signal_like_metrics() {
        assert!(Metric::Signal(vec![]).default_thresholds().is_some());
        assert!(Metric::Sir(vec![]).default_thresholds().is_some());
        assert!(Metric::ApCount { min_rssi_dbm: -80 }.default_thresholds().is_none());
    }
}
