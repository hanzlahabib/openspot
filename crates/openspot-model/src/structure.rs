//! Project structure above zones: buildings (ordered floors), floor alignment and the
//! created/changed timestamps shown in the project explorer.

use crate::{Project, Uuid};

/// Default height of a floor in meters (typical storey height); only used for elevation.
pub const DEFAULT_FLOOR_HEIGHT_M: f64 = 3.0;

/// Rigid transform that places one floor's plan into the building's shared coordinates
/// (meters): rotate about the floor origin, then translate. Used to line floors up
/// when comparing them, e.g. a stairwell appearing at the same spot on every level.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FloorAlignment {
    pub dx_m: f64,
    pub dy_m: f64,
    pub rotation_rad: f64,
}

impl FloorAlignment {
    /// Floor-local meters to building meters.
    pub fn to_building(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (s, c) = self.rotation_rad.sin_cos();
        (x * c - y * s + self.dx_m, x * s + y * c + self.dy_m)
    }

    /// Building meters back to floor-local meters (inverse of [`Self::to_building`]).
    pub fn from_building(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (s, c) = self.rotation_rad.sin_cos();
        let (x, y) = (x - self.dx_m, y - self.dy_m);
        (x * c + y * s, -x * s + y * c)
    }
}

/// One storey of a building: the zone that holds its plan plus where it sits vertically.
#[derive(Debug, Clone, PartialEq)]
pub struct Floor {
    pub zone: Uuid,
    /// Level label shown in the explorer, e.g. "Ground", "1", "Basement".
    pub label: String,
    pub height_m: f64,
    pub alignment: FloorAlignment,
}

/// A building groups zones as floors. `floors` run from the lowest level to the highest.
#[derive(Debug, Clone, PartialEq)]
pub struct Building {
    pub id: Uuid,
    pub name: String,
    pub floors: Vec<Floor>,
}

/// Buildings and project-level metadata. A zone belongs to at most one building.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Structure {
    pub buildings: Vec<Building>,
    /// Unix seconds (UTC). 0 means unknown (a project written before this was recorded).
    pub created: i64,
    pub changed: i64,
}

/// What the explorer needs to list a project file without loading its measurements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummary {
    pub name: String,
    pub created: i64,
    pub changed: i64,
    pub zone_count: usize,
}

impl Structure {
    /// Records a save at `now`: sets the creation time the first time, always the change time.
    pub fn stamp_saved(&mut self, now: i64) {
        if self.created == 0 {
            self.created = now;
        }
        self.changed = now;
    }

    pub fn building_of(&self, zone: Uuid) -> Option<usize> {
        self.buildings.iter().position(|b| b.floors.iter().any(|f| f.zone == zone))
    }

    pub fn add_building(&mut self, name: impl Into<String>) -> Uuid {
        let id = Uuid::new_v4();
        self.buildings.push(Building { id, name: name.into(), floors: Vec::new() });
        id
    }

    /// Deletes a building; its zones simply become ungrouped.
    pub fn remove_building(&mut self, id: Uuid) -> bool {
        let before = self.buildings.len();
        self.buildings.retain(|b| b.id != id);
        self.buildings.len() != before
    }

    pub fn rename_building(&mut self, id: Uuid, name: &str) -> bool {
        let name = name.trim();
        match self.buildings.iter_mut().find(|b| b.id == id) {
            Some(b) if !name.is_empty() => {
                b.name = name.to_owned();
                true
            }
            _ => false,
        }
    }

    /// Puts `zone` on top of building `building`, taking it out of any other building
    /// first ("move zone to building"). Returns false for an unknown building.
    pub fn move_zone_to_building(&mut self, zone: Uuid, building: Uuid) -> bool {
        if !self.buildings.iter().any(|b| b.id == building) {
            return false;
        }
        self.forget_zone(zone);
        let Some(b) = self.buildings.iter_mut().find(|b| b.id == building) else { return false };
        b.floors.push(Floor {
            zone,
            label: format!("Floor {}", b.floors.len() + 1),
            height_m: DEFAULT_FLOOR_HEIGHT_M,
            alignment: FloorAlignment::default(),
        });
        true
    }

    /// Puts `new_zone` directly above `after` in the same building (a duplicated floor).
    /// Does nothing when `after` is not in a building.
    pub fn add_floor_above(&mut self, after: Uuid, new_zone: Uuid) {
        for b in &mut self.buildings {
            if let Some(i) = b.floors.iter().position(|f| f.zone == after) {
                let mut floor = b.floors[i].clone();
                floor.zone = new_zone;
                floor.label = format!("{} copy", floor.label);
                b.floors.insert(i + 1, floor);
                return;
            }
        }
    }

    /// Removes `zone` from whichever building holds it (also used when a zone is deleted).
    pub fn forget_zone(&mut self, zone: Uuid) {
        for b in &mut self.buildings {
            b.floors.retain(|f| f.zone != zone);
        }
    }

    /// Moves a floor within its building. Indices are positions in `floors`.
    pub fn reorder_floor(&mut self, building: Uuid, from: usize, to: usize) -> bool {
        let Some(b) = self.buildings.iter_mut().find(|b| b.id == building) else { return false };
        if from >= b.floors.len() || to >= b.floors.len() {
            return false;
        }
        let floor = b.floors.remove(from);
        b.floors.insert(to, floor);
        true
    }

    pub fn floor_mut(&mut self, zone: Uuid) -> Option<&mut Floor> {
        self.buildings.iter_mut().flat_map(|b| b.floors.iter_mut()).find(|f| f.zone == zone)
    }

    /// Height of the floor's base above the building's lowest floor, in meters.
    pub fn elevation_m(&self, building: Uuid, floor_index: usize) -> Option<f64> {
        let b = self.buildings.iter().find(|b| b.id == building)?;
        if floor_index >= b.floors.len() {
            return None;
        }
        Some(b.floors[..floor_index].iter().map(|f| f.height_m).sum())
    }

    /// Drops floors whose zone no longer exists, so a hand-edited or partially restored
    /// file never leaves dangling references.
    pub fn prune(&mut self, zones: &[crate::Zone]) {
        for b in &mut self.buildings {
            b.floors.retain(|f| zones.iter().any(|z| z.id == f.zone));
        }
    }
}

/// A zone in the explorer tree with its snapshots by index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneNode {
    pub zone_index: usize,
    pub name: String,
    /// Level label when the zone is a building floor.
    pub level: Option<String>,
    /// `(snapshot index, name)`.
    pub snapshots: Vec<(usize, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildingNode {
    pub id: Uuid,
    pub name: String,
    /// Highest floor first, as a building is read top-down.
    pub zones: Vec<ZoneNode>,
}

/// Explorer tree: project > building > zone > snapshot. Zones outside any building are
/// listed in `ungrouped`, in project order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outline {
    pub buildings: Vec<BuildingNode>,
    pub ungrouped: Vec<ZoneNode>,
}

impl Project {
    pub fn outline(&self) -> Outline {
        let node = |zone_index: usize, level: Option<String>| {
            let z = &self.zones[zone_index];
            ZoneNode {
                zone_index,
                name: z.name.clone(),
                level,
                snapshots: z.snapshots.iter().enumerate().map(|(i, s)| (i, s.name.clone())).collect(),
            }
        };
        let buildings = self
            .structure
            .buildings
            .iter()
            .map(|b| BuildingNode {
                id: b.id,
                name: b.name.clone(),
                zones: b
                    .floors
                    .iter()
                    .rev()
                    .filter_map(|f| self.zones.iter().position(|z| z.id == f.zone).map(|i| node(i, Some(f.label.clone()))))
                    .collect(),
            })
            .collect();
        let ungrouped = (0..self.zones.len())
            .filter(|&i| self.structure.building_of(self.zones[i].id).is_none())
            .map(|i| node(i, None))
            .collect();
        Outline { buildings, ungrouped }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Zone;

    fn project_with_zones(n: usize) -> Project {
        let mut p = Project::new("p");
        for i in 1..n {
            p.zones.push(Zone::new(format!("Zone {}", i + 1)));
        }
        p
    }

    #[test]
    fn alignment_round_trips_and_rotates() {
        let a = FloorAlignment { dx_m: 2.0, dy_m: -1.0, rotation_rad: std::f64::consts::FRAC_PI_2 };
        // A quarter turn maps +x to +y before the translation.
        let (x, y) = a.to_building((1.0, 0.0));
        assert!((x - 2.0).abs() < 1e-12 && (y - 0.0).abs() < 1e-12, "({x}, {y})");
        let p = (3.7, -4.2);
        let back = a.from_building(a.to_building(p));
        assert!((back.0 - p.0).abs() < 1e-12 && (back.1 - p.1).abs() < 1e-12);
    }

    #[test]
    fn moving_zone_between_buildings_keeps_single_membership() {
        let p = project_with_zones(2);
        let mut s = Structure::default();
        let (a, b) = (s.add_building("A"), s.add_building("B"));
        let z = p.zones[0].id;
        assert!(s.move_zone_to_building(z, a));
        assert_eq!(s.building_of(z), Some(0));
        assert!(s.move_zone_to_building(z, b));
        assert_eq!(s.building_of(z), Some(1));
        assert!(s.buildings[0].floors.is_empty());
        assert!(!s.move_zone_to_building(z, Uuid::new_v4()));
        assert_eq!(s.building_of(z), Some(1), "failed move must not unassign");
    }

    #[test]
    fn reorder_and_elevation() {
        let p = project_with_zones(3);
        let mut s = Structure::default();
        let b = s.add_building("B");
        for z in &p.zones {
            s.move_zone_to_building(z.id, b);
        }
        s.floor_mut(p.zones[1].id).unwrap().height_m = 4.5;
        assert_eq!(s.elevation_m(b, 2), Some(DEFAULT_FLOOR_HEIGHT_M + 4.5));
        assert!(s.reorder_floor(b, 2, 0));
        let order: Vec<_> = s.buildings[0].floors.iter().map(|f| f.zone).collect();
        assert_eq!(order, vec![p.zones[2].id, p.zones[0].id, p.zones[1].id]);
        assert_eq!(s.elevation_m(b, 2), Some(2.0 * DEFAULT_FLOOR_HEIGHT_M));
        assert!(!s.reorder_floor(b, 3, 0));
        assert_eq!(s.elevation_m(b, 3), None);
    }

    #[test]
    fn outline_lists_top_floor_first_and_ungrouped_zones() {
        let mut p = project_with_zones(3);
        let b = p.structure.add_building("Tower");
        let (z0, z1) = (p.zones[0].id, p.zones[1].id);
        p.structure.move_zone_to_building(z0, b);
        p.structure.move_zone_to_building(z1, b);
        p.structure.floor_mut(z1).unwrap().label = "Roof".into();
        let o = p.outline();
        assert_eq!(o.buildings.len(), 1);
        let names: Vec<_> = o.buildings[0].zones.iter().map(|z| (z.zone_index, z.level.clone().unwrap())).collect();
        assert_eq!(names, vec![(1, "Roof".to_owned()), (0, "Floor 1".to_owned())]);
        assert_eq!(o.ungrouped.len(), 1);
        assert_eq!(o.ungrouped[0].zone_index, 2);
        assert_eq!(o.ungrouped[0].snapshots[0].1, "Snapshot 1");
    }

    #[test]
    fn prune_drops_floors_of_missing_zones_and_forget_removes_on_delete() {
        let mut p = project_with_zones(2);
        let b = p.structure.add_building("B");
        let (z0, z1) = (p.zones[0].id, p.zones[1].id);
        p.structure.move_zone_to_building(z0, b);
        p.structure.move_zone_to_building(z1, b);
        p.zones.pop();
        p.structure.prune(&p.zones);
        assert_eq!(p.structure.buildings[0].floors.len(), 1);
        p.structure.forget_zone(z0);
        assert!(p.structure.buildings[0].floors.is_empty());
    }

    #[test]
    fn duplicated_floor_lands_above_original() {
        let mut p = project_with_zones(3);
        let b = p.structure.add_building("B");
        let (z0, z1, z2) = (p.zones[0].id, p.zones[1].id, p.zones[2].id);
        p.structure.move_zone_to_building(z0, b);
        p.structure.move_zone_to_building(z1, b);
        p.structure.add_floor_above(z0, z2);
        let order: Vec<_> = p.structure.buildings[0].floors.iter().map(|f| f.zone).collect();
        assert_eq!(order, vec![z0, z2, z1]);
        assert_eq!(p.structure.buildings[0].floors[1].label, "Floor 1 copy");
        // An ungrouped original leaves the copy ungrouped too.
        let mut s = Structure::default();
        s.add_floor_above(z0, z2);
        assert_eq!(s.building_of(z2), None);
    }

    #[test]
    fn stamp_keeps_creation_time() {
        let mut s = Structure::default();
        s.stamp_saved(100);
        s.stamp_saved(200);
        assert_eq!((s.created, s.changed), (100, 200));
    }

    #[test]
    fn rename_rejects_blank() {
        let mut s = Structure::default();
        let b = s.add_building("A");
        assert!(!s.rename_building(b, "  "));
        assert!(s.rename_building(b, " Main "));
        assert_eq!(s.buildings[0].name, "Main");
        assert!(s.remove_building(b));
        assert!(!s.remove_building(b));
    }
}
