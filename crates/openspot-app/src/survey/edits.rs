//! Undoable structural edits: undo/redo itself plus the snapshot and zone-name operations that
//! feed it. Positions are indices, which stays valid because every change to the zone and
//! snapshot lists is either recorded (so undo restores the old layout first, last-in
//! first-out) or clears the history.

use openspot_model::{Scale, Snapshot, SnapshotCanvas, Uuid};

use super::Survey;
use super::history::Edit;

impl Survey {
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undoes the last edit (point, scale, snapshot add/delete/merge or a rename). Returns
    /// whether anything changed.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.history.pop_undo() else { return false };
        match &edit {
            Edit::PointAdded { zone, snapshot, point } => {
                if let Some(snap) = self.project.zones.get_mut(*zone).and_then(|z| z.snapshots.get_mut(*snapshot)) {
                    snap.points.retain(|p| p.id != point.id);
                }
            }
            Edit::SnapshotInserted { zone, snapshot, .. } => self.remove_snapshot_by_id(*zone, snapshot.id, true),
            Edit::SnapshotDeleted { zone, index, snapshot, canvas } => {
                self.insert_snapshot_at(*zone, *index, snapshot.clone(), canvas.clone());
            }
            Edit::SnapshotRenamed { zone, snapshot, before, .. } => self.set_snapshot_name(*zone, *snapshot, before),
            Edit::ZoneRenamed { zone, before, .. } => self.set_zone_name(*zone, before),
            Edit::ScaleSet { zone, before, .. } => {
                if let Some(z) = self.project.zones.get_mut(*zone) {
                    z.scale = *before;
                }
            }
        }
        self.history.push_redo(edit);
        self.touch();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.history.pop_redo() else { return false };
        match &edit {
            Edit::PointAdded { zone, snapshot, point } => {
                if let Some(snap) = self.project.zones.get_mut(*zone).and_then(|z| z.snapshots.get_mut(*snapshot)) {
                    snap.points.push(point.clone());
                }
            }
            Edit::SnapshotInserted { zone, index, snapshot, canvas } => {
                self.insert_snapshot_at(*zone, *index, snapshot.clone(), canvas.clone());
            }
            Edit::SnapshotDeleted { zone, snapshot, .. } => self.remove_snapshot_by_id(*zone, snapshot.id, false),
            Edit::SnapshotRenamed { zone, snapshot, after, .. } => self.set_snapshot_name(*zone, *snapshot, after),
            Edit::ZoneRenamed { zone, after, .. } => self.set_zone_name(*zone, after),
            Edit::ScaleSet { zone, after, .. } => {
                if let Some(z) = self.project.zones.get_mut(*zone) {
                    z.scale = *after;
                }
            }
        }
        self.history.push_undo(edit);
        self.touch();
        true
    }

    pub fn set_scale(&mut self, scale: Scale) {
        let zone = self.zone;
        let before = self.zone().scale;
        self.zone_mut().scale = Some(scale);
        self.history.record(Edit::ScaleSet { zone, before, after: Some(scale) });
        self.touch();
    }

    /// Renames the current zone. Blank names are rejected so the explorer never shows an
    /// empty label.
    pub fn rename_zone(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let zone = self.zone;
        let before = self.zone().name.clone();
        if before != name {
            self.history.record(Edit::ZoneRenamed { zone, before, after: name.to_owned() });
        }
        self.set_zone_name(zone, name);
        self.touch();
        true
    }

    pub fn rename_snapshot(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let (zone, snapshot) = (self.zone, self.snapshot);
        let before = self.snapshot().name.clone();
        if before != name {
            self.history.record(Edit::SnapshotRenamed { zone, snapshot, before, after: name.to_owned() });
        }
        self.set_snapshot_name(zone, snapshot, name);
        self.touch();
        true
    }

    /// Appends an empty snapshot to the current zone and selects it.
    pub fn add_snapshot(&mut self) {
        let snapshot = Snapshot::new(format!("Snapshot {}", self.zone().snapshots.len() + 1));
        let (zone, index) = (self.zone, self.zone().snapshots.len());
        self.history.record(Edit::SnapshotInserted { zone, index, snapshot: snapshot.clone(), canvas: SnapshotCanvas::default() });
        self.insert_snapshot_at(zone, index, snapshot, SnapshotCanvas::default());
        self.touch();
    }

    /// Deletes the current snapshot. A zone always keeps one snapshot. Undo brings it back
    /// whole, unless it is so large that keeping a copy would break the history's memory
    /// bound; then the message says so and the history starts over.
    pub fn delete_snapshot(&mut self) -> bool {
        if self.zone().snapshots.len() <= 1 {
            return false;
        }
        let (zone, index) = (self.zone, self.snapshot);
        let snapshot = self.zone().snapshots[index].clone();
        let canvas = self.project.canvas.snapshot(snapshot.id).cloned().unwrap_or_default();
        let id = snapshot.id;
        let kept = self.history.record(Edit::SnapshotDeleted { zone, index, snapshot, canvas });
        self.remove_snapshot_by_id(zone, id, false);
        if !kept {
            self.message = Some("Snapshot deleted; it is too large to undo".to_owned());
        }
        self.touch();
        true
    }

    fn set_zone_name(&mut self, zone: usize, name: &str) {
        if let Some(z) = self.project.zones.get_mut(zone) {
            name.clone_into(&mut z.name);
        }
    }

    fn set_snapshot_name(&mut self, zone: usize, snapshot: usize, name: &str) {
        if let Some(s) = self.project.zones.get_mut(zone).and_then(|z| z.snapshots.get_mut(snapshot)) {
            name.clone_into(&mut s.name);
        }
    }

    /// Puts `snapshot` (and its canvas data) at `index` of `zone`, shifting later ones, and
    /// selects it when that zone is showing.
    pub(super) fn insert_snapshot_at(&mut self, zone: usize, index: usize, snapshot: Snapshot, canvas: SnapshotCanvas) {
        let Some(z) = self.project.zones.get_mut(zone) else { return };
        let index = index.min(z.snapshots.len());
        self.project.canvas.set_snapshot(snapshot.id, canvas);
        z.snapshots.insert(index, snapshot);
        if zone == self.zone {
            self.snapshot = index;
            self.selected.clear();
            self.per_ap = None;
        }
    }

    /// Removes the snapshot with `id` from `zone` (matched by id, so a stale index cannot
    /// remove the wrong one) and keeps the selection valid. A zone's last snapshot is never
    /// removed. After removal the view moves to the previous snapshot when `prefer_previous`
    /// (undoing an add) and otherwise to the one that took its place (deleting).
    pub(super) fn remove_snapshot_by_id(&mut self, zone: usize, id: Uuid, prefer_previous: bool) {
        let Some(z) = self.project.zones.get_mut(zone) else { return };
        let Some(at) = z.snapshots.iter().position(|s| s.id == id) else { return };
        if z.snapshots.len() <= 1 {
            return;
        }
        z.snapshots.remove(at);
        let last = z.snapshots.len() - 1;
        self.project.canvas.remove_snapshot(id);
        if zone != self.zone {
            return;
        }
        let shown = self.snapshot;
        if shown > at {
            self.snapshot = shown - 1;
        } else if shown == at {
            self.snapshot = if prefer_previous { at.saturating_sub(1) } else { at.min(last) };
            self.selected.clear();
            self.per_ap = None;
        }
        self.snapshot = self.snapshot.min(last);
    }
}
