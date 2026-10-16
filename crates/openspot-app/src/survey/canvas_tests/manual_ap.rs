use super::*;
use crate::survey::ap_manual::hit_marker;

const AP: MacAddress = MacAddress([0, 0, 0, 0, 0, 1]);

fn has_manual(s: &Survey) -> bool {
    s.project.canvas.snapshot(s.snapshot().id).is_some_and(|c| c.manual_aps.contains_key(&AP))
}

/// Four corners hear the AP: the automatic estimate lands near (100, 100).
fn survey() -> Survey {
    let mut s = Survey::default();
    s.create_blank_map(20.0, 20.0).unwrap();
    for (x, y) in [(0.0, 0.0), (200.0, 0.0), (0.0, 200.0), (200.0, 200.0)] {
        s.add_scan(x, y, vec![ap(1, -60)]);
    }
    s.selected.insert(AP);
    s
}

#[test]
fn manual_position_overrides_the_estimate_and_reset_restores_it() {
    let mut s = survey();
    let est = s.ap_markers();
    assert_eq!(est.len(), 1);
    assert!(!est[0].manual);
    assert!((est[0].x - 100.0).abs() < 1e-6 && (est[0].y - 100.0).abs() < 1e-6);

    s.set_manual_ap(AP, 40.0, 50.0);
    let m = &s.ap_markers()[0];
    assert!(m.manual);
    assert_eq!((m.x, m.y), (40.0, 50.0));
    assert!(has_manual(&s));

    assert!(s.reset_manual_ap(&AP));
    assert!(!s.ap_markers()[0].manual);
    assert!(!s.reset_manual_ap(&AP), "nothing left to reset");
}

#[test]
fn manual_position_works_without_enough_readings_for_an_estimate() {
    let mut s = Survey::default();
    s.create_blank_map(20.0, 20.0).unwrap();
    s.selected.insert(AP);
    assert!(s.ap_markers().is_empty());
    s.set_manual_ap(AP, 10.0, 10.0);
    assert_eq!(s.ap_markers().len(), 1);
}

#[test]
fn positions_are_clamped_to_the_plan_and_garbage_is_ignored() {
    let mut s = survey();
    s.set_manual_ap(AP, -50.0, 1.0e9);
    let m = s.ap_markers()[0].clone();
    let map = s.zone().map.as_ref().unwrap();
    assert_eq!((m.x, m.y), (0.0, f64::from(map.height)));
    s.set_manual_ap(AP, f64::NAN, 5.0);
    assert_eq!((s.ap_markers()[0].x, s.ap_markers()[0].y), (m.x, m.y));
}

#[test]
fn drag_follows_the_pointer_but_only_commits_on_release() {
    let mut s = survey();
    assert!(!s.begin_ap_drag(MacAddress([9; 6])), "unknown or unselected AP cannot be dragged");
    assert!(s.begin_ap_drag(AP));
    s.move_ap_drag(30.0, 40.0);
    let m = &s.ap_markers()[0];
    assert_eq!((m.x, m.y, m.manual), (30.0, 40.0, true));
    assert!(!has_manual(&s), "not stored mid-drag");
    s.end_ap_drag();
    assert!(has_manual(&s));
    assert!(s.ap_drag.is_none());
    assert_eq!((s.ap_markers()[0].x, s.ap_markers()[0].y), (30.0, 40.0));
}

#[test]
fn cancelled_drag_leaves_no_trace() {
    let mut s = survey();
    s.begin_ap_drag(AP);
    s.move_ap_drag(1.0, 1.0);
    s.cancel_ap_drag();
    assert!(!has_manual(&s));
    assert!(!s.ap_markers()[0].manual);
}

#[test]
fn positions_are_per_snapshot_and_persist() {
    let mut s = survey();
    s.set_manual_ap(AP, 12.0, 34.0);
    s.add_snapshot();
    s.selected.insert(AP);
    assert!(!has_manual(&s));
    s.select_snapshot(0);
    let dir = std::env::temp_dir().join(format!("openspot-ap-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    s.save_to(&dir.join("a.ospot")).unwrap();
    let mut t = Survey::default();
    t.open_from(&dir.join("a.ospot")).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    t.selected.insert(AP);
    let m = &t.ap_markers()[0];
    assert_eq!((m.x, m.y, m.manual), (12.0, 34.0, true));
}

#[test]
fn reset_all_clears_every_manual_position() {
    let mut s = survey();
    s.set_manual_ap(AP, 1.0, 1.0);
    s.set_manual_ap(MacAddress([0, 0, 0, 0, 0, 2]), 2.0, 2.0);
    assert!(s.snapshot_has_manual_aps());
    assert_eq!(s.reset_all_manual_aps(), 2);
    assert!(!s.snapshot_has_manual_aps());
}

#[test]
fn hit_test_picks_the_nearest_marker_within_the_radius() {
    let a = MacAddress([0, 0, 0, 0, 0, 1]);
    let b = MacAddress([0, 0, 0, 0, 0, 2]);
    let markers = [(a, (100.0, 100.0)), (b, (108.0, 100.0))];
    assert_eq!(hit_marker(&markers, (106.0, 100.0), 14.0), Some(b));
    assert_eq!(hit_marker(&markers, (96.0, 100.0), 14.0), Some(a));
    assert_eq!(hit_marker(&markers, (200.0, 200.0), 14.0), None);
    assert_eq!(hit_marker(&[], (0.0, 0.0), 14.0), None);
}
