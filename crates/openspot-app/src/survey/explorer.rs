//! Project-structure operations on [`Survey`]: duplicating zones and snapshots.

use std::path::Path;

use openspot_model::Uuid;

use super::projects::{ProjectRow, RecentEntry, SortKey, push_recent};
use super::{Survey, now_unix};

/// UI state of the project explorer panel.
#[derive(Debug)]
pub struct ExplorerState {
    pub visible: bool,
    /// The start page replaces the survey canvas until a project is opened or created.
    pub start_page: bool,
    pub recent: Vec<RecentEntry>,
    /// Metadata rows of the recent files; `None` until loaded, reset to refresh.
    pub rows: Option<Vec<ProjectRow>>,
    pub sort: SortKey,
    pub ascending: bool,
    /// Text of the "new building" field.
    pub new_building: String,
    /// Building being renamed, with the edited text.
    pub rename: Option<(Uuid, String)>,
}

impl Default for ExplorerState {
    fn default() -> Self {
        Self {
            visible: false,
            start_page: true,
            recent: Vec::new(),
            rows: None,
            sort: SortKey::default(),
            ascending: SortKey::default().starts_ascending(),
            new_building: String::new(),
            rename: None,
        }
    }
}

impl Survey {
    /// Inserts a deep copy of the current zone right after it and selects the copy.
    pub fn duplicate_zone(&mut self) {
        let copy = self.zone().duplicate();
        self.project.structure.add_floor_above(self.zone().id, copy.id);
        self.project.zones.insert(self.zone + 1, copy);
        // Edits refer to zones by index, which just shifted.
        self.history.clear();
        let new = self.zone + 1;
        self.select_zone(new);
        self.touch();
    }

    /// Remembers `path` as the most recently used project file.
    pub(super) fn note_recent(&mut self, path: &Path) {
        push_recent(&mut self.explorer.recent, path, now_unix());
        self.explorer.rows = None;
    }

    /// Selects a zone and snapshot from the explorer tree.
    pub fn select_node(&mut self, zone: usize, snapshot: Option<usize>) {
        self.select_zone(zone);
        if let Some(i) = snapshot {
            self.select_snapshot(i);
        }
    }

    /// Adds a building named `name` (blank names are ignored). Returns its id.
    pub fn add_building(&mut self, name: &str) -> Option<Uuid> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let id = self.project.structure.add_building(name);
        self.touch();
        Some(id)
    }

    pub fn move_zone_to_building(&mut self, zone: usize, building: Uuid) {
        let Some(id) = self.project.zones.get(zone).map(|z| z.id) else { return };
        if self.project.structure.move_zone_to_building(id, building) {
            self.touch();
        }
    }

    pub fn ungroup_zone(&mut self, zone: usize) {
        if let Some(id) = self.project.zones.get(zone).map(|z| z.id) {
            self.project.structure.forget_zone(id);
            self.touch();
        }
    }

    pub fn remove_building(&mut self, building: Uuid) {
        if self.project.structure.remove_building(building) {
            self.touch();
        }
    }

    pub fn rename_building(&mut self, building: Uuid, name: &str) {
        if self.project.structure.rename_building(building, name) {
            self.touch();
        }
    }

    /// Moves the floor of `zone` one step up (`delta > 0`) or down inside its building.
    pub fn shift_floor(&mut self, zone: usize, delta: isize) {
        let Some(id) = self.project.zones.get(zone).map(|z| z.id) else { return };
        let Some(bi) = self.project.structure.building_of(id) else { return };
        let b = &self.project.structure.buildings[bi];
        let Some(from) = b.floors.iter().position(|f| f.zone == id) else { return };
        let Some(to) = from.checked_add_signed(delta) else { return };
        let building = b.id;
        if self.project.structure.reorder_floor(building, from, to) {
            self.touch();
        }
    }

    /// Inserts a deep copy of the current snapshot right after it and selects the copy.
    pub fn duplicate_snapshot(&mut self) {
        let copy = self.snapshot().duplicate();
        let at = self.snapshot + 1;
        self.zone_mut().snapshots.insert(at, copy);
        self.history.clear();
        self.select_snapshot(at);
        self.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_zone_selects_independent_copy() {
        let mut s = Survey::default();
        s.add_scan(1.0, 2.0, vec![]);
        let original = s.zone().id;
        s.duplicate_zone();
        assert_eq!(s.project.zones.len(), 2);
        assert_eq!(s.zone, 1);
        assert_ne!(s.zone().id, original);
        assert_eq!(s.zone().name, "Zone 1 copy");
        assert_eq!(s.snapshot().points.len(), 1);
        s.add_scan(3.0, 3.0, vec![]);
        assert_eq!(s.project.zones[0].snapshots[0].points.len(), 1);
        assert!(s.dirty);
    }

    #[test]
    fn duplicate_zone_joins_the_same_building_and_delete_leaves_it() {
        let mut s = Survey::default();
        let b = s.add_building(" Tower ").unwrap();
        s.move_zone_to_building(0, b);
        s.duplicate_zone();
        let floors = &s.project.structure.buildings[0].floors;
        assert_eq!(floors.len(), 2);
        assert_eq!(floors[1].zone, s.zone().id);
        assert!(s.delete_zone());
        assert_eq!(s.project.structure.buildings[0].floors.len(), 1);
        assert!(s.add_building("  ").is_none());
    }

    #[test]
    fn select_node_drives_zone_and_snapshot() {
        let mut s = Survey::default();
        s.add_zone();
        s.add_snapshot();
        s.select_node(0, Some(0));
        assert_eq!((s.zone, s.snapshot), (0, 0));
        s.select_node(1, Some(1));
        assert_eq!((s.zone, s.snapshot), (1, 1));
    }

    #[test]
    fn shift_floor_reorders_within_building() {
        let mut s = Survey::default();
        s.add_zone();
        let b = s.add_building("B").unwrap();
        s.move_zone_to_building(0, b);
        s.move_zone_to_building(1, b);
        s.shift_floor(1, -1);
        let first = s.project.structure.buildings[0].floors[0].zone;
        assert_eq!(first, s.project.zones[1].id);
        s.shift_floor(1, 5); // out of range: ignored
        assert_eq!(s.project.structure.buildings[0].floors[0].zone, first);
    }

    #[test]
    fn duplicate_snapshot_is_inserted_after_current() {
        let mut s = Survey::default();
        s.add_scan(1.0, 2.0, vec![]);
        s.duplicate_snapshot();
        assert_eq!(s.zone().snapshots.len(), 2);
        assert_eq!(s.snapshot, 1);
        assert_eq!(s.snapshot().name, "Snapshot 1 copy");
        assert_eq!(s.snapshot().points[0].x, 1.0);
        assert_ne!(s.snapshot().points[0].id, s.zone().snapshots[0].points[0].id);
    }

    #[test]
    fn save_and_open_feed_the_recent_list_and_new_keeps_it() {
        let dir = std::env::temp_dir().join(format!("openspot-recent-flow-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.ospot");
        let mut s = Survey::default();
        s.save_to(&path).unwrap();
        assert_eq!(s.explorer.recent.len(), 1);
        assert!(s.project.structure.created > 0, "first save records the creation time");
        s.new_project();
        assert_eq!(s.explorer.recent.len(), 1, "new project keeps recents");
        assert!(!s.explorer.start_page);
        s.explorer.start_page = true;
        s.open_from(&path).unwrap();
        assert!(!s.explorer.start_page);
        assert_eq!(s.explorer.recent.len(), 1, "reopening does not duplicate");
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }
}
