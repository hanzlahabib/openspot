//! Undo/redo of survey edits.
//!
//! Edits are recorded as small operations (not project snapshots) because a project can
//! embed multi-megabyte floor plans. Map loads and zone deletion are deliberately not
//! undoable: a plan image is megabytes, so keeping the previous one per edit would defeat the
//! memory bound, and both clear the history instead (see `Survey::install_map`).

use std::collections::VecDeque;

use openspot_model::{ScanPoint, Scale, Snapshot, SnapshotCanvas};

/// Most edits kept for undo, however small.
const MAX_HISTORY: usize = 500;

/// Memory budget for kept edits, in "records" (one per scan point, access point or small
/// edit; see [`Edit::cost`]). An access point record with its raw information elements is
/// roughly half a kilobyte, so this bounds the history to about 25 MB. Deleting a snapshot
/// stores the whole snapshot, which is why a size budget and not only a count is needed.
pub const MAX_COST: usize = 50_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    PointAdded { zone: usize, snapshot: usize, point: ScanPoint },
    /// A snapshot inserted into a zone (a new empty one, or the result of a merge). Undo
    /// removes it again.
    SnapshotInserted { zone: usize, index: usize, snapshot: Snapshot, canvas: SnapshotCanvas },
    /// A snapshot deleted from a zone, kept whole so undo can bring it back.
    SnapshotDeleted { zone: usize, index: usize, snapshot: Snapshot, canvas: SnapshotCanvas },
    SnapshotRenamed { zone: usize, snapshot: usize, before: String, after: String },
    ZoneRenamed { zone: usize, before: String, after: String },
    ScaleSet { zone: usize, before: Option<Scale>, after: Option<Scale> },
}

impl Edit {
    /// Rough memory weight in records: one for the edit itself plus one per scan point and
    /// access point it holds.
    pub fn cost(&self) -> usize {
        match self {
            Self::PointAdded { point, .. } => point_cost(point),
            Self::SnapshotInserted { snapshot, .. } | Self::SnapshotDeleted { snapshot, .. } => {
                1 + snapshot.points.iter().map(point_cost).sum::<usize>()
            }
            Self::SnapshotRenamed { .. } | Self::ZoneRenamed { .. } | Self::ScaleSet { .. } => 1,
        }
    }
}

fn point_cost(point: &ScanPoint) -> usize {
    1 + point.access_points.len()
}

#[derive(Debug, Default)]
pub struct History {
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    /// Sum of [`Edit::cost`] over `undo` and `redo`.
    cost: usize,
}

impl History {
    /// Records a new edit; this invalidates anything that could have been redone. Returns
    /// `false` when the edit alone exceeds the memory budget: it is then not kept, and since
    /// older edits refer to positions it may have shifted, the whole history is dropped.
    pub fn record(&mut self, edit: Edit) -> bool {
        self.drop_redo();
        let cost = edit.cost();
        if cost > MAX_COST {
            self.clear();
            return false;
        }
        self.cost += cost;
        self.undo.push_back(edit);
        while self.undo.len() > MAX_HISTORY || self.cost > MAX_COST {
            // The budget check above guarantees the newest edit fits, so this cannot empty the stack.
            if let Some(old) = self.undo.pop_front() {
                self.cost -= old.cost();
            }
        }
        true
    }

    fn drop_redo(&mut self) {
        self.cost -= self.redo.iter().map(Edit::cost).sum::<usize>();
        self.redo.clear();
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.cost = 0;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn pop_undo(&mut self) -> Option<Edit> {
        self.undo.pop_back()
    }

    pub fn pop_redo(&mut self) -> Option<Edit> {
        self.redo.pop()
    }

    /// Moves an edit that was just undone onto the redo stack.
    pub fn push_redo(&mut self, edit: Edit) {
        self.redo.push(edit);
    }

    /// Moves an edit that was just redone back onto the undo stack (keeps redo intact).
    pub fn push_undo(&mut self, edit: Edit) {
        self.undo.push_back(edit);
    }

    /// Current memory weight of all kept edits.
    #[cfg(test)]
    pub fn cost(&self) -> usize {
        self.cost
    }
}

#[cfg(test)]
mod tests {
    use openspot_wlan::{AccessPoint, MacAddress};

    use super::*;

    fn point(x: f64) -> Edit {
        Edit::PointAdded { zone: 0, snapshot: 0, point: ScanPoint::new(x, 0.0, 0, vec![]) }
    }

    fn heavy_snapshot(points: usize, aps_per_point: usize) -> Snapshot {
        let ap = AccessPoint::from_beacon("n".into(), MacAddress([0; 6]), -50, 2_437_000, true, 100, vec![]);
        let mut s = Snapshot::new("big");
        for i in 0..points {
            s.points.push(ScanPoint::new(i as f64, 0.0, 0, vec![ap.clone(); aps_per_point]));
        }
        s
    }

    fn deleted(snapshot: Snapshot) -> Edit {
        Edit::SnapshotDeleted { zone: 0, index: 1, snapshot, canvas: SnapshotCanvas::default() }
    }

    #[test]
    fn record_clears_redo_and_stacks_are_lifo() {
        let mut h = History::default();
        assert!(!h.can_undo() && !h.can_redo());
        h.record(point(1.0));
        h.record(point(2.0));
        let e = h.pop_undo().unwrap();
        h.push_redo(e.clone());
        assert!(h.can_redo());
        assert!(matches!(e, Edit::PointAdded { ref point, .. } if point.x == 2.0));
        h.record(point(3.0));
        assert!(!h.can_redo(), "a new edit invalidates redo");
    }

    #[test]
    fn invalidated_redo_edits_stop_counting_towards_the_budget() {
        let mut h = History::default();
        h.record(deleted(heavy_snapshot(10, 9)));
        let before = h.cost();
        let e = h.pop_undo().unwrap();
        h.push_redo(e);
        assert_eq!(h.cost(), before, "moving between stacks keeps the weight");
        h.record(point(1.0));
        assert_eq!(h.cost(), 1, "the new edit replaced the redo stack");
    }

    #[test]
    fn history_is_bounded_by_count() {
        let mut h = History::default();
        for i in 0..MAX_HISTORY + 20 {
            h.record(point(i as f64));
        }
        let mut n = 0;
        while h.pop_undo().is_some() {
            n += 1;
        }
        assert_eq!(n, MAX_HISTORY);
    }

    #[test]
    fn history_is_bounded_by_weight_dropping_the_oldest_first() {
        let mut h = History::default();
        // Each deleted snapshot weighs 1 + 100 * (1 + 49) = 5001.
        let per = deleted(heavy_snapshot(100, 49)).cost();
        let fit = MAX_COST / per;
        for _ in 0..fit + 3 {
            assert!(h.record(deleted(heavy_snapshot(100, 49))));
        }
        assert!(h.cost() <= MAX_COST, "{} > {MAX_COST}", h.cost());
        let mut n = 0;
        while h.pop_undo().is_some() {
            n += 1;
        }
        assert_eq!(n, fit, "only as many edits as fit are kept");
    }

    #[test]
    fn an_edit_larger_than_the_budget_is_refused_and_drops_stale_history() {
        let mut h = History::default();
        h.record(point(1.0));
        assert!(!h.record(deleted(heavy_snapshot(MAX_COST, 1))));
        assert!(!h.can_undo() && !h.can_redo(), "older edits could point at shifted positions");
        assert_eq!(h.cost(), 0);
    }

    #[test]
    fn costs_count_points_and_access_points() {
        assert_eq!(point(0.0).cost(), 1);
        assert_eq!(deleted(heavy_snapshot(3, 2)).cost(), 1 + 3 * 3);
        let rename = Edit::ZoneRenamed { zone: 0, before: "a".into(), after: "b".into() };
        assert_eq!(rename.cost(), 1);
    }
}
