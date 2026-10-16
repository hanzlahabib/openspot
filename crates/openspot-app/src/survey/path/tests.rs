use super::*;

const EPS: f64 = 1e-9;

fn near(a: Point, b: Point) -> bool {
    (a.0 - b.0).abs() < EPS && (a.1 - b.1).abs() < EPS
}

fn assert_points(got: &[Point], want: &[Point]) {
    assert_eq!(got.len(), want.len(), "got {got:?}, want {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(near(*g, *w), "got {got:?}, want {want:?}");
    }
}

#[test]
fn straight_line_exact_multiple_has_both_ends_once() {
    assert_points(&sample_along(&[(0.0, 0.0), (10.0, 0.0)], 5.0), &[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)]);
}

#[test]
fn remainder_adds_the_end_point() {
    assert_points(&sample_along(&[(0.0, 0.0), (10.0, 0.0)], 4.0), &[(0.0, 0.0), (4.0, 0.0), (8.0, 0.0), (10.0, 0.0)]);
}

#[test]
fn step_longer_than_route_gives_start_and_end() {
    assert_points(&sample_along(&[(0.0, 0.0), (3.0, 4.0)], 100.0), &[(0.0, 0.0), (3.0, 4.0)]);
}

#[test]
fn arc_length_is_continuous_around_a_corner() {
    // L-shape, 20 long: samples every 4 along the route, not restarting at the corner.
    let got = sample_along(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], 4.0);
    assert_points(&got, &[(0.0, 0.0), (4.0, 0.0), (8.0, 0.0), (10.0, 2.0), (10.0, 6.0), (10.0, 10.0)]);
}

#[test]
fn consecutive_samples_are_one_step_apart_along_the_route() {
    let route = [(0.0, 0.0), (30.0, 0.0), (30.0, 40.0)];
    let got = sample_along(&route, 7.0);
    // Every sample except the last sits on a multiple of 7 of arc length.
    for (k, p) in got.iter().take(got.len() - 1).enumerate() {
        let d = k as f64 * 7.0;
        let want = if d <= 30.0 { (d, 0.0) } else { (30.0, d - 30.0) };
        assert!(near(*p, want), "sample {k}: {p:?} vs {want:?}");
    }
    assert!(near(got[got.len() - 1], (30.0, 40.0)));
    assert_eq!(got.len(), 70 / 7 + 1);
}

#[test]
fn duplicate_waypoints_and_zero_length_segments_are_ignored() {
    let a = sample_along(&[(0.0, 0.0), (0.0, 0.0), (6.0, 0.0), (6.0, 0.0), (6.0, 0.0)], 3.0);
    let b = sample_along(&[(0.0, 0.0), (6.0, 0.0)], 3.0);
    assert_points(&a, &b);
}

#[test]
fn degenerate_inputs() {
    assert!(sample_along(&[], 1.0).is_empty());
    assert_points(&sample_along(&[(5.0, 5.0)], 1.0), &[(5.0, 5.0)]);
    assert_points(&sample_along(&[(5.0, 5.0), (5.0, 5.0)], 1.0), &[(5.0, 5.0)]);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(sample_along(&[(0.0, 0.0), (1.0, 0.0)], bad).is_empty(), "{bad}");
    }
}

#[test]
fn tiny_segment_does_not_produce_duplicate_end() {
    // Total 10.000000000001: the last regular sample is within tolerance of the end.
    let got = sample_along(&[(0.0, 0.0), (10.0, 0.0), (10.000_000_000_001, 0.0)], 5.0);
    assert_eq!(got.len(), 3);
}

#[test]
fn many_samples_accumulate_no_drift() {
    let got = sample_along(&[(0.0, 0.0), (1000.0, 0.0)], 0.1);
    assert_eq!(got.len(), 10_001);
    assert!(near(got[10_000], (1000.0, 0.0)));
    assert!((got[3333].0 - 333.3).abs() < 1e-9);
}

fn plan(n: usize) -> PathPlan {
    PathPlan { targets: (0..n).map(|i| (i as f64, 0.0)).collect(), step_secs: 2.0, warning: None }
}

#[test]
fn first_sample_is_due_immediately_then_waits_for_the_pace() {
    let mut run = PathRun::start(plan(3), 100.0);
    assert_eq!(run.poll(100.0), Some(DueSample { index: 0, x: 0.0, y: 0.0 }));
    assert_eq!(run.poll(100.5), None, "scan still in flight");
    run.scan_finished(true);
    assert_eq!(run.poll(101.0), None, "sample 1 due at +2 s");
    assert_eq!(run.poll(102.0), Some(DueSample { index: 1, x: 1.0, y: 0.0 }));
    run.scan_finished(true);
    assert_eq!(run.progress(), (2, 3));
    assert_eq!(run.poll(103.9), None);
    assert!(run.poll(104.0).is_some());
    run.scan_finished(true);
    assert_eq!(run.state(), RunState::Finished);
    assert_eq!(run.poll(500.0), None);
}

#[test]
fn a_slow_scan_delays_but_never_skips_samples() {
    let mut run = PathRun::start(plan(3), 0.0);
    assert!(run.poll(0.0).is_some());
    // Scan takes 5 s: samples 1 (due 2 s) is overdue, sample 2 (due 4 s) too.
    assert_eq!(run.poll(5.0), None);
    run.scan_finished(true);
    assert_eq!(run.poll(5.0).map(|d| d.index), Some(1));
    run.scan_finished(true);
    assert_eq!(run.poll(5.0).map(|d| d.index), Some(2));
}

#[test]
fn pause_stops_the_clock_and_resume_continues() {
    let mut run = PathRun::start(plan(3), 0.0);
    assert!(run.poll(0.0).is_some());
    run.scan_finished(true);
    run.poll(1.0);
    run.pause(1.0);
    assert_eq!(run.state(), RunState::Paused);
    assert_eq!(run.poll(50.0), None, "paused");
    run.resume(50.0);
    assert_eq!(run.poll(50.5), None, "only 1.5 s of run time so far");
    assert_eq!(run.poll(51.0).map(|d| d.index), Some(1));
}

#[test]
fn failed_scan_pauses_and_resume_retries_the_same_sample() {
    let mut run = PathRun::start(plan(2), 0.0);
    assert_eq!(run.poll(0.0).map(|d| d.index), Some(0));
    run.scan_finished(false);
    assert_eq!(run.state(), RunState::Paused);
    assert_eq!(run.progress(), (0, 2));
    run.resume(10.0);
    assert_eq!(run.poll(10.0).map(|d| d.index), Some(0));
}

#[test]
fn plan_uses_scale_for_spacing_and_pace() {
    let scale = Scale::new(10.0).unwrap(); // 10 px per meter
    let p = PathPlan::new(&[(0.0, 0.0), (100.0, 0.0)], Some(scale), 2.0, 0.5).unwrap();
    assert_eq!(p.targets.len(), 6, "10 m route every 2 m");
    assert!(near(p.targets[1], (20.0, 0.0)));
    assert!((p.step_secs - 4.0).abs() < EPS);
    assert!(p.warning.is_none());
}

#[test]
fn plan_without_scale_falls_back_to_pixels_with_a_warning() {
    let p = PathPlan::new(&[(0.0, 0.0), (120.0, 0.0)], None, 2.0, 0.5).unwrap();
    assert_eq!(p.targets.len(), 3);
    assert!(near(p.targets[1], (FALLBACK_INTERVAL_PX, 0.0)));
    assert_eq!(p.step_secs, FALLBACK_STEP_SECS);
    assert!(p.warning.is_some());
}

#[test]
fn plan_rejects_bad_input() {
    let scale = Scale::new(10.0);
    assert_eq!(PathPlan::new(&[(0.0, 0.0)], scale, 1.0, 1.0), Err(PathError::TooShort));
    assert_eq!(PathPlan::new(&[(1.0, 1.0), (1.0, 1.0)], scale, 1.0, 1.0), Err(PathError::TooShort));
    assert_eq!(PathPlan::new(&[(0.0, 0.0), (9.0, 0.0)], scale, 0.0, 1.0), Err(PathError::BadParameters));
    assert_eq!(PathPlan::new(&[(0.0, 0.0), (9.0, 0.0)], scale, 1.0, f64::NAN), Err(PathError::BadParameters));
    assert_eq!(PathPlan::new(&[(0.0, 0.0), (100_000.0, 0.0)], scale, 0.1, 1.0), Err(PathError::TooManySamples));
}
