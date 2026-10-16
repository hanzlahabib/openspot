use super::*;
use crate::survey::history::MAX_COST;

fn names(s: &Survey) -> Vec<String> {
    s.zone().snapshots.iter().map(|x| x.name.clone()).collect()
}

#[test]
fn adding_a_snapshot_is_undoable_and_redo_brings_back_the_same_one() {
    let mut s = Survey::default();
    s.add_snapshot();
    let id = s.snapshot().id;
    assert_eq!((s.snapshot, s.zone().snapshots.len()), (1, 2));
    assert!(s.undo());
    assert_eq!((s.snapshot, s.zone().snapshots.len()), (0, 1), "back on the snapshot the user came from");
    assert!(s.redo());
    assert_eq!((s.snapshot, s.snapshot().id), (1, id));
}

#[test]
fn deleting_a_snapshot_is_undoable_with_points_position_and_canvas_data() {
    let mut s = Survey::default();
    s.add_snapshot();
    s.add_snapshot();
    s.select_snapshot(1);
    s.add_scan(5.0, 6.0, vec![ap(1, -50), ap(2, -60)]);
    let id = s.snapshot().id;
    s.project.canvas.snapshot_mut(id).manual_aps.insert(MacAddress([0, 0, 0, 0, 0, 1]), (1.0, 2.0));
    let before = s.project.clone();

    assert!(s.delete_snapshot());
    assert_eq!(names(&s), ["Snapshot 1", "Snapshot 3"]);
    assert!(s.project.canvas.snapshot(id).is_none(), "its canvas data goes with it");

    assert!(s.undo());
    assert_eq!(names(&s), ["Snapshot 1", "Snapshot 2", "Snapshot 3"], "restored in its old position");
    assert_eq!(s.snapshot, 1, "the restored snapshot is shown");
    assert_eq!(s.project, before, "points, ids and canvas data come back unchanged");

    assert!(s.redo());
    assert_eq!(names(&s), ["Snapshot 1", "Snapshot 3"]);
    assert!(s.project.canvas.snapshot(id).is_none());
}

#[test]
fn mixed_edits_unwind_in_order_back_to_the_start() {
    let mut s = Survey::default();
    let start = s.project.clone();
    s.add_snapshot();
    s.add_scan(1.0, 1.0, vec![ap(1, -50)]);
    assert!(s.rename_snapshot("Walk test"));
    s.select_snapshot(0);
    s.add_scan(2.0, 2.0, vec![ap(1, -60)]);
    assert!(s.rename_zone("Ground"));
    s.select_snapshot(1);
    assert!(s.delete_snapshot());
    let end = s.project.clone();

    let mut undone = 0;
    while s.undo() {
        undone += 1;
    }
    assert_eq!(undone, 6, "one step per edit");
    assert_eq!(s.project, start);
    let mut redone = 0;
    while s.redo() {
        redone += 1;
    }
    assert_eq!(redone, 6);
    assert_eq!(s.project, end);
}

#[test]
fn renames_undo_and_redo() {
    let mut s = Survey::default();
    let (zone, snapshot) = (s.zone().name.clone(), s.snapshot().name.clone());
    assert!(s.rename_zone("  Ground floor "));
    assert!(s.rename_snapshot("Before move"));
    assert!(s.undo());
    assert_eq!((s.zone().name.as_str(), s.snapshot().name.as_str()), ("Ground floor", snapshot.as_str()));
    assert!(s.undo());
    assert_eq!(s.zone().name, zone);
    assert!(s.redo() && s.redo());
    assert_eq!((s.zone().name.as_str(), s.snapshot().name.as_str()), ("Ground floor", "Before move"));
}

#[test]
fn rejected_and_unchanged_renames_are_not_edits() {
    let mut s = Survey::default();
    assert!(!s.rename_zone("  "));
    let same = s.zone().name.clone();
    assert!(s.rename_zone(&same));
    let same = s.snapshot().name.clone();
    assert!(s.rename_snapshot(&same));
    assert!(!s.can_undo());
}

#[test]
fn a_new_edit_after_undo_invalidates_redo() {
    let mut s = Survey::default();
    s.add_snapshot();
    assert!(s.undo() && s.can_redo());
    assert!(s.rename_zone("Other"));
    assert!(!s.can_redo(), "redoing the add would now be out of order");
    assert_eq!(s.zone().snapshots.len(), 1);
}

#[test]
fn undoing_a_snapshot_edit_made_in_another_zone_leaves_the_view_alone() {
    let mut s = Survey::default();
    s.add_snapshot();
    s.add_zone();
    assert_eq!((s.zone, s.snapshot), (1, 0));
    assert!(s.undo());
    assert_eq!(s.project.zones[0].snapshots.len(), 1);
    assert_eq!((s.zone, s.snapshot), (1, 0));
}

#[test]
fn undoing_an_add_while_viewing_an_earlier_snapshot_keeps_that_snapshot() {
    let mut s = Survey::default();
    s.add_snapshot();
    s.add_snapshot();
    s.select_snapshot(0);
    let first = s.snapshot().id;
    assert!(s.undo());
    assert_eq!(s.zone().snapshots.len(), 2);
    assert_eq!(s.snapshot().id, first);
}

#[test]
fn structural_changes_that_are_not_undoable_clear_the_history() {
    let mut s = Survey::default();
    s.add_snapshot();
    s.add_zone();
    s.add_zone();
    assert!(s.delete_zone());
    assert!(!s.can_undo(), "zone indices shifted");

    s.add_snapshot();
    assert!(s.can_undo());
    s.create_blank_map(10.0, 10.0).unwrap();
    assert!(!s.can_undo() && !s.can_redo(), "map loads are not undoable");
}

#[test]
fn deleting_a_snapshot_too_large_to_keep_says_so_and_resets_history() {
    let mut s = Survey::default();
    s.add_snapshot();
    let big = vec![ap(1, -50); MAX_COST];
    s.add_scan(1.0, 1.0, big);
    s.add_snapshot();
    s.select_snapshot(1);
    assert!(s.delete_snapshot());
    assert_eq!(s.zone().snapshots.len(), 2);
    assert!(!s.can_undo() && !s.can_redo());
    assert!(s.message.as_deref().is_some_and(|m| m.contains("too large to undo")), "{:?}", s.message);
}

#[test]
fn undo_and_redo_mark_the_project_changed() {
    let mut s = Survey::default();
    s.add_snapshot();
    s.dirty = false;
    let generation = s.generation;
    assert!(s.undo());
    assert!(s.dirty && s.generation > generation, "the heatmap cache is invalidated");
}
