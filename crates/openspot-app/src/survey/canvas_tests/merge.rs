use super::*;
use crate::survey::merge::MergeError;

fn survey() -> Survey {
    let mut s = Survey::default();
    s.create_blank_map(100.0, 50.0).unwrap(); // 16 px/m -> merge epsilon 8 px
    s.add_scan(100.0, 100.0, vec![ap(1, -50)]);
    s.add_scan(300.0, 100.0, vec![ap(1, -60)]);
    s.add_snapshot();
    s.add_scan(103.0, 101.0, vec![ap(1, -52)]); // within 8 px of A's first point
    s.add_scan(500.0, 500.0, vec![ap(1, -70)]);
    s.select_snapshot(0);
    s
}

#[test]
fn merge_creates_a_new_snapshot_and_keeps_the_sources() {
    let mut s = survey();
    let idx = s.merge_selected_snapshots(&[1, 0]).unwrap();
    assert_eq!(idx, 2);
    assert_eq!(s.snapshot, 2, "the merged snapshot is selected");
    assert_eq!(s.zone().snapshots.len(), 3);
    assert_eq!(s.zone().snapshots[0].points.len(), 2);
    assert_eq!(s.zone().snapshots[1].points.len(), 2);
    let merged = s.snapshot();
    assert_eq!(merged.name, "Merged: Snapshot 1 + Snapshot 2");
    assert_eq!(merged.points.len(), 3, "one duplicate removed");
    let first = &merged.points[0];
    assert_eq!((first.x, first.access_points[0].rssi_dbm), (100.0, -50), "earlier snapshot wins");
}

#[test]
fn provenance_counts_are_recorded() {
    let mut s = survey();
    s.merge_selected_snapshots(&[0, 1]).unwrap();
    let p = s.snapshot_provenance();
    assert_eq!(p.len(), 2);
    assert_eq!((p[0].points_in, p[0].points_kept), (2, 2));
    assert_eq!((p[1].points_in, p[1].points_kept), (2, 1));
}

#[test]
fn needs_two_distinct_existing_snapshots() {
    let mut s = survey();
    assert_eq!(s.merge_selected_snapshots(&[0]), Err(MergeError::TooFew));
    assert_eq!(s.merge_selected_snapshots(&[1, 1]), Err(MergeError::TooFew));
    assert_eq!(s.merge_selected_snapshots(&[0, 9]), Err(MergeError::Missing));
    assert_eq!(s.zone().snapshots.len(), 2, "failed merges change nothing");
}

#[test]
fn undo_removes_the_merged_snapshot_and_redo_restores_it_with_provenance() {
    let mut s = survey();
    s.merge_selected_snapshots(&[0, 1]).unwrap();
    let id = s.snapshot().id;
    assert!(s.undo());
    assert_eq!(s.zone().snapshots.len(), 2);
    assert!(s.snapshot < 2);
    assert!(s.project.canvas.snapshot(id).is_none());
    assert!(s.redo());
    assert_eq!(s.zone().snapshots.len(), 3);
    assert_eq!(s.zone().snapshots[2].id, id);
    s.select_snapshot(2);
    assert_eq!(s.snapshot_provenance().len(), 2);
}

#[test]
fn merged_snapshot_survives_save_and_open_with_unique_point_ids() {
    let mut s = survey();
    s.merge_selected_snapshots(&[0, 1]).unwrap();
    let dir = std::env::temp_dir().join(format!("openspot-merge-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    s.save_to(&dir.join("m.ospot")).unwrap(); // would fail on duplicate point ids
    let mut t = Survey::default();
    t.open_from(&dir.join("m.ospot")).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    t.select_snapshot(2);
    assert_eq!(t.snapshot().points.len(), 3);
    assert_eq!(t.snapshot_provenance().len(), 2);
}

#[test]
fn without_a_scale_the_fallback_distance_applies() {
    let mut s = Survey::default();
    assert_eq!(s.merge_epsilon_px(), crate::survey::merge::MERGE_FALLBACK_EPSILON_PX);
    s.set_scale(Scale::new(20.0).unwrap());
    assert_eq!(s.merge_epsilon_px(), 10.0);
}
