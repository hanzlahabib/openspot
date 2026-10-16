//! Reversible edits of planning data and their undo/redo history.
//!
//! Like Survey's history, edits are small operations rather than project snapshots, because
//! a project can embed multi-megabyte floor plans. Zones are referred to by id so an edit
//! survives zones being added, removed or reordered in Survey.

use openspot_model::{Cutout, Material, PlanningData, Project, Slab, Uuid, VirtualAp, Wall};

/// Edits kept for undo. Each is small; this just bounds memory for very long sessions.
const MAX_HISTORY: usize = 500;

/// One list element added (`before` is `None`), removed (`after` is `None`) or replaced.
#[derive(Debug, Clone, PartialEq)]
pub struct Change<T> {
    pub index: usize,
    pub before: Option<T>,
    pub after: Option<T>,
}

impl<T: Clone> Change<T> {
    pub fn added(index: usize, item: T) -> Self {
        Self { index, before: None, after: Some(item) }
    }

    pub fn removed(index: usize, item: T) -> Self {
        Self { index, before: Some(item), after: None }
    }

    pub fn modified(index: usize, before: T, after: T) -> Self {
        Self { index, before: Some(before), after: Some(after) }
    }

    /// Applies the change (`forward`) or its inverse. False when the list does not look
    /// like the state the change was recorded against.
    fn apply(&self, list: &mut Vec<T>, forward: bool) -> bool {
        let (from, to) = if forward { (&self.before, &self.after) } else { (&self.after, &self.before) };
        match (from, to) {
            (None, Some(item)) if self.index <= list.len() => {
                list.insert(self.index, item.clone());
                true
            }
            (Some(_), None) if self.index < list.len() => {
                list.remove(self.index);
                true
            }
            (Some(_), Some(item)) => match list.get_mut(self.index) {
                Some(slot) => {
                    *slot = item.clone();
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Wall { zone: Uuid, change: Change<Wall> },
    Ap { zone: Uuid, change: Change<VirtualAp> },
    Cutout { zone: Uuid, change: Change<Cutout> },
    Material { zone: Uuid, change: Change<Material> },
    Slab { zone: Uuid, before: Slab, after: Slab },
    /// Several edits that undo and redo together.
    Batch(Vec<Edit>),
}

impl Edit {
    /// Applies the edit to `project`, or reverts it when `forward` is false.
    pub fn apply(&self, project: &mut Project, forward: bool) -> bool {
        match self {
            Self::Batch(edits) => {
                if forward {
                    edits.iter().all(|e| e.apply(project, true))
                } else {
                    edits.iter().rev().all(|e| e.apply(project, false))
                }
            }
            Self::Wall { zone, change } => with_plan(project, *zone, |p| change.apply(&mut p.walls, forward)),
            Self::Ap { zone, change } => with_plan(project, *zone, |p| change.apply(&mut p.access_points, forward)),
            Self::Cutout { zone, change } => with_plan(project, *zone, |p| change.apply(&mut p.cutouts, forward)),
            Self::Material { zone, change } => with_plan(project, *zone, |p| change.apply(&mut p.materials, forward)),
            Self::Slab { zone, before, after } => with_plan(project, *zone, |p| {
                p.level.slab = if forward { after.clone() } else { before.clone() };
                true
            }),
        }
    }
}

fn with_plan(project: &mut Project, zone: Uuid, f: impl FnOnce(&mut PlanningData) -> bool) -> bool {
    project.zones.iter_mut().find(|z| z.id == zone).is_some_and(|z| f(&mut z.planning))
}

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl History {
    /// Records an edit that was just applied; anything that could have been redone is gone.
    pub fn record(&mut self, edit: Edit) {
        self.redo.clear();
        self.undo.push(edit);
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Reverts the latest edit. An edit that no longer applies (its zone was deleted in
    /// Survey) is dropped and reported as nothing changed.
    pub fn undo(&mut self, project: &mut Project) -> bool {
        let Some(edit) = self.undo.pop() else { return false };
        let ok = edit.apply(project, false);
        if ok {
            self.redo.push(edit);
        }
        ok
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        let Some(edit) = self.redo.pop() else { return false };
        let ok = edit.apply(project, true);
        if ok {
            self.undo.push(edit);
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::{Point, WallKind, Zone};

    fn project() -> (Project, Uuid) {
        let p = Project::new("t");
        let id = p.zones[0].id;
        (p, id)
    }

    fn wall(x: f64) -> Wall {
        Wall::new(Point::new(x, 0.0), Point::new(x, 3.0), WallKind::Wall, "drywall")
    }

    fn add(p: &mut Project, h: &mut History, zone: Uuid, w: Wall) {
        let index = p.zones[0].planning.walls.len();
        let edit = Edit::Wall { zone, change: Change::added(index, w) };
        assert!(edit.apply(p, true));
        h.record(edit);
    }

    #[test]
    fn undo_redo_restore_exact_states_in_order() {
        let (mut p, z) = project();
        let mut h = History::default();
        let (w1, w2) = (wall(1.0), wall(2.0));
        add(&mut p, &mut h, z, w1.clone());
        add(&mut p, &mut h, z, w2.clone());
        let two = p.zones[0].planning.clone();

        let mut modified = w1.clone();
        modified.material = "concrete-wall".into();
        let edit = Edit::Wall { zone: z, change: Change::modified(0, w1.clone(), modified.clone()) };
        assert!(edit.apply(&mut p, true));
        h.record(edit);
        assert_eq!(p.zones[0].planning.walls[0].material, "concrete-wall");

        assert!(h.undo(&mut p));
        assert_eq!(p.zones[0].planning, two, "modification reverted");
        assert!(h.undo(&mut p) && h.undo(&mut p));
        assert!(p.zones[0].planning.walls.is_empty());
        assert!(!h.undo(&mut p), "nothing left to undo");
        assert!(h.redo(&mut p) && h.redo(&mut p));
        assert_eq!(p.zones[0].planning, two);
        assert!(h.redo(&mut p));
        assert_eq!(p.zones[0].planning.walls[0], modified);
        assert!(!h.can_redo());
    }

    #[test]
    fn removal_restores_the_wall_at_its_old_index() {
        let (mut p, z) = project();
        let mut h = History::default();
        for x in [1.0, 2.0, 3.0] {
            add(&mut p, &mut h, z, wall(x));
        }
        let middle = p.zones[0].planning.walls[1].clone();
        let edit = Edit::Wall { zone: z, change: Change::removed(1, middle.clone()) };
        assert!(edit.apply(&mut p, true));
        h.record(edit);
        assert_eq!(p.zones[0].planning.walls.len(), 2);
        assert!(h.undo(&mut p));
        assert_eq!(p.zones[0].planning.walls[1], middle);
    }

    #[test]
    fn a_new_edit_invalidates_redo_and_history_is_bounded() {
        let (mut p, z) = project();
        let mut h = History::default();
        add(&mut p, &mut h, z, wall(1.0));
        assert!(h.undo(&mut p));
        assert!(h.can_redo());
        add(&mut p, &mut h, z, wall(2.0));
        assert!(!h.can_redo());
        for i in 0..MAX_HISTORY + 20 {
            add(&mut p, &mut h, z, wall(i as f64));
        }
        let mut n = 0;
        while h.undo(&mut p) {
            n += 1;
        }
        assert_eq!(n, MAX_HISTORY);
    }

    #[test]
    fn batch_is_atomic_for_undo_and_slab_edits_round_trip() {
        let (mut p, z) = project();
        let mut h = History::default();
        let before = p.zones[0].planning.level.slab.clone();
        let after = Slab { thickness_m: 0.3, material: "wood-floor".into() };
        let batch = Edit::Batch(vec![
            Edit::Wall { zone: z, change: Change::added(0, wall(1.0)) },
            Edit::Wall { zone: z, change: Change::added(1, wall(2.0)) },
            Edit::Slab { zone: z, before: before.clone(), after: after.clone() },
        ]);
        assert!(batch.apply(&mut p, true));
        h.record(batch);
        assert_eq!((p.zones[0].planning.walls.len(), &p.zones[0].planning.level.slab), (2, &after));
        assert!(h.undo(&mut p));
        assert_eq!((p.zones[0].planning.walls.len(), &p.zones[0].planning.level.slab), (0, &before));
        assert!(h.redo(&mut p));
        assert_eq!(p.zones[0].planning.walls.len(), 2);
    }

    #[test]
    fn edits_for_a_deleted_zone_are_dropped_instead_of_corrupting_another() {
        let (mut p, z) = project();
        p.zones.push(Zone::new("other"));
        let mut h = History::default();
        add(&mut p, &mut h, z, wall(1.0));
        p.zones.remove(0);
        assert!(!h.undo(&mut p));
        assert!(p.zones[0].planning.walls.is_empty());
        assert!(!h.can_undo() && !h.can_redo(), "the stale edit is gone");
    }

    #[test]
    fn stale_indices_are_refused() {
        let (mut p, z) = project();
        let bad = Edit::Wall { zone: z, change: Change::removed(3, wall(1.0)) };
        assert!(!bad.apply(&mut p, true));
        let bad = Edit::Wall { zone: z, change: Change::modified(0, wall(1.0), wall(2.0)) };
        assert!(!bad.apply(&mut p, true));
    }
}
