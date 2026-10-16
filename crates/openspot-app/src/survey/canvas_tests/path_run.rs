use super::*;
use crate::survey::path::RunState;

/// Survey with a 10 px/m scale and a route 0..20 m long at 1 m/s, sampled every 10 m.
fn survey_with_route() -> Survey {
    let mut s = Survey::default();
    s.set_scale(Scale::new(10.0).unwrap());
    s.walk_speed_mps = 1.0;
    s.set_path_interval_m(10.0);
    assert!(s.path_click(0.0, 0.0) && s.path_click(200.0, 0.0));
    s
}

#[test]
fn run_records_points_in_route_order_on_the_clock() {
    let mut s = survey_with_route();
    s.path_start(0.0).unwrap();
    s.drive_path(0.0);
    assert_eq!(s.take_pending_scan(), Some((0.0, 0.0)));
    assert!(s.busy);
    s.on_scan_result(0.0, 0.0, Ok(vec![ap(1, -50)]), Vec::new());
    s.drive_path(5.0);
    assert_eq!(s.take_pending_scan(), None, "second sample is due at 10 s");
    s.drive_path(10.0);
    assert_eq!(s.take_pending_scan(), Some((100.0, 0.0)));
    s.on_scan_result(100.0, 0.0, Ok(vec![ap(1, -60)]), Vec::new());
    s.drive_path(20.0);
    assert_eq!(s.take_pending_scan(), Some((200.0, 0.0)));
    s.on_scan_result(200.0, 0.0, Ok(vec![ap(1, -70)]), Vec::new());
    let xs: Vec<f64> = s.snapshot().points.iter().map(|p| p.x).collect();
    assert_eq!(xs, [0.0, 100.0, 200.0]);
    assert!(s.path_run.is_none(), "finished run is cleared");
    assert!(s.message.as_deref().unwrap().contains("3 points"));
}

#[test]
fn failed_scan_pauses_and_resume_retries_without_a_gap() {
    let mut s = survey_with_route();
    s.path_start(0.0).unwrap();
    s.drive_path(0.0);
    s.take_pending_scan();
    s.on_scan_result(0.0, 0.0, Err("adapter busy".into()), Vec::new());
    assert!(s.snapshot().points.is_empty());
    assert_eq!(s.path_run.as_ref().unwrap().run.state(), RunState::Paused);
    s.drive_path(50.0);
    assert_eq!(s.take_pending_scan(), None, "paused");
    s.path_resume(50.0);
    s.drive_path(50.0);
    assert_eq!(s.take_pending_scan(), Some((0.0, 0.0)), "same sample again");
}

#[test]
fn cancel_stops_requests_and_keeps_recorded_points() {
    let mut s = survey_with_route();
    s.path_start(0.0).unwrap();
    s.drive_path(0.0);
    s.take_pending_scan();
    s.on_scan_result(0.0, 0.0, Ok(vec![ap(1, -50)]), Vec::new());
    s.path_cancel();
    s.drive_path(100.0);
    assert_eq!(s.take_pending_scan(), None);
    assert_eq!(s.snapshot().points.len(), 1);
    // A scan that was already in flight still records its valid reading, but does not
    // revive the run.
    s.on_scan_result(100.0, 0.0, Ok(vec![ap(1, -55)]), Vec::new());
    assert_eq!(s.snapshot().points.len(), 2);
    s.drive_path(200.0);
    assert_eq!(s.take_pending_scan(), None);
    assert!(!s.path_running());
}

#[test]
fn switching_snapshot_cancels_the_run() {
    let mut s = survey_with_route();
    s.path_start(0.0).unwrap();
    s.add_snapshot();
    s.drive_path(0.0);
    assert!(s.path_run.is_none());
    assert_eq!(s.take_pending_scan(), None);
}

#[test]
fn route_is_locked_while_running_and_is_per_snapshot() {
    let mut s = survey_with_route();
    s.path_start(0.0).unwrap();
    assert!(!s.path_click(5.0, 5.0) && !s.path_clear() && !s.path_remove_last());
    assert_eq!(s.path_waypoints().len(), 2);
    s.path_cancel();
    assert!(s.path_remove_last());
    assert_eq!(s.path_waypoints().len(), 1);
    s.add_snapshot();
    assert!(s.path_waypoints().is_empty(), "a new snapshot has its own route");
}

#[test]
fn manual_scan_in_progress_defers_the_run() {
    let mut s = survey_with_route();
    s.path_start(0.0).unwrap();
    s.busy = true;
    s.drive_path(0.0);
    assert_eq!(s.take_pending_scan(), None);
    s.busy = false;
    s.drive_path(1.0);
    assert!(s.take_pending_scan().is_some());
}

#[test]
fn start_requires_a_route_and_warns_without_scale() {
    let mut s = Survey::default();
    assert!(s.path_start(0.0).is_err());
    s.path_click(0.0, 0.0);
    s.path_click(300.0, 0.0);
    s.path_start(0.0).unwrap();
    assert!(s.message.as_deref().unwrap().contains("No scale"));
}

#[test]
fn route_survives_save_and_open() {
    let mut s = survey_with_route();
    let dir = std::env::temp_dir().join(format!("openspot-path-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("p.ospot");
    s.save_to(&file).unwrap();
    let mut t = Survey::default();
    t.open_from(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    assert_eq!(t.path_waypoints(), [(0.0, 0.0), (200.0, 0.0)]);
    assert_eq!(t.path_interval_m(), 10.0);
}
