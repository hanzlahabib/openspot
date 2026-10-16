//! The stack of levels a prediction runs over: the floors of the active zone's building,
//! bottom to top, with elevations and alignment taken from the project structure.
//!
//! A zone outside any building is a single level. Predicting a floor includes the access
//! points and walls of the other floors of its building, so slab loss and cross-floor
//! coverage are real.

use openspot_model::{PlanningData, Point, Project, Uuid};

/// Why a stack could not be built.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StackError {
    #[error("floor '{0}' has a height of zero or less; set the floor heights of the building")]
    BadHeight(String),
    #[error("no such zone")]
    NoZone,
}

/// Levels ready for the engine, bottom to top.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelStack {
    /// Zone of each level.
    pub zones: Vec<Uuid>,
    /// Level labels for the UI ("Ground", or the zone name outside a building).
    pub labels: Vec<String>,
    /// Planning data with elevation, floor height and alignment offset filled in from the
    /// structure. Stored zone data is not modified.
    pub levels: Vec<PlanningData>,
    /// Index of the zone being predicted.
    pub active: usize,
    /// Things the user should know that do not stop a prediction.
    pub warnings: Vec<String>,
}

/// Rotation (radians) below which a floor alignment counts as unrotated: 0.001 rad is
/// 0.06 degrees, under 6 cm over 60 m, below drawing accuracy.
const NEGLIGIBLE_ROTATION_RAD: f64 = 1e-3;

impl LevelStack {
    /// Index (in `project.zones`) of the floor `delta` steps above (negative: below) zone
    /// `zone_index` in its building. `None` outside a building or past either end.
    pub fn neighbor(project: &Project, zone_index: usize, delta: isize) -> Option<usize> {
        let zone = project.zones.get(zone_index)?.id;
        let building = project.structure.buildings.get(project.structure.building_of(zone)?)?;
        let at = building.floors.iter().position(|f| f.zone == zone)? as isize;
        let target = building.floors.get(usize::try_from(at + delta).ok()?)?.zone;
        project.zones.iter().position(|z| z.id == target)
    }

    pub fn build(project: &Project, zone_index: usize) -> Result<Self, StackError> {
        let active_zone = project.zones.get(zone_index).ok_or(StackError::NoZone)?;
        let zone = |id: Uuid| project.zones.iter().find(|z| z.id == id);

        let Some(building) = project.structure.building_of(active_zone.id).and_then(|b| project.structure.buildings.get(b)) else {
            let mut data = active_zone.planning.clone();
            data.level.elevation_m = 0.0;
            return Ok(Self { zones: vec![active_zone.id], labels: vec![active_zone.name.clone()], levels: vec![data], active: 0, warnings: Vec::new() });
        };

        let (mut zones, mut labels, mut levels, mut warnings) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut elevation = 0.0;
        let mut active = 0;
        for floor in &building.floors {
            let Some(z) = zone(floor.zone) else { continue };
            if !(floor.height_m.is_finite() && floor.height_m > 0.0) {
                return Err(StackError::BadHeight(floor.label.clone()));
            }
            let mut data = z.planning.clone();
            data.level.elevation_m = elevation;
            data.level.floor_height_m = floor.height_m;
            if floor.alignment.rotation_rad.abs() > NEGLIGIBLE_ROTATION_RAD {
                warnings.push(format!("floor '{}' is rotated in the building; rotation is not applied to predictions, only its offset", floor.label));
            }
            data.level.offset = Point::new(data.level.offset.x + floor.alignment.dx_m, data.level.offset.y + floor.alignment.dy_m);
            if z.id == active_zone.id {
                active = levels.len();
            }
            zones.push(z.id);
            labels.push(floor.label.clone());
            levels.push(data);
            elevation += floor.height_m;
        }
        Ok(Self { zones, labels, levels, active, warnings })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::{FloorAlignment, Zone};

    fn building(heights: &[f64]) -> Project {
        let mut p = Project::new("p");
        while p.zones.len() < heights.len() {
            p.zones.push(Zone::new(format!("z{}", p.zones.len())));
        }
        let b = p.structure.add_building("HQ");
        for z in p.zones.clone() {
            p.structure.move_zone_to_building(z.id, b);
        }
        for (f, h) in p.structure.buildings[0].floors.iter_mut().zip(heights) {
            f.height_m = *h;
        }
        p
    }

    #[test]
    fn a_lone_zone_is_one_level_at_the_ground() {
        let mut p = Project::new("p");
        p.zones[0].planning.level.elevation_m = 12.0;
        let s = LevelStack::build(&p, 0).unwrap();
        assert_eq!((s.levels.len(), s.active, s.levels[0].level.elevation_m), (1, 0, 0.0));
        assert_eq!(s.labels, vec!["Zone 1"]);
        assert_eq!(LevelStack::build(&p, 3), Err(StackError::NoZone));
    }

    #[test]
    fn building_floors_stack_by_their_heights_from_the_structure() {
        let p = building(&[4.0, 3.0, 2.5]);
        let s = LevelStack::build(&p, 1).unwrap();
        let elev: Vec<f64> = s.levels.iter().map(|l| l.level.elevation_m).collect();
        assert_eq!(elev, vec![0.0, 4.0, 7.0]);
        assert_eq!(s.levels.iter().map(|l| l.level.floor_height_m).collect::<Vec<_>>(), vec![4.0, 3.0, 2.5]);
        assert_eq!((s.active, s.zones[s.active]), (1, p.zones[1].id));
        assert!(s.warnings.is_empty());
        // The engine accepts it: strictly increasing elevations.
        let refs: Vec<&PlanningData> = s.levels.iter().collect();
        assert!(openspot_predict::Scene::build(&refs, &openspot_model::MaterialCatalog::builtin().unwrap()).is_ok());
    }

    #[test]
    fn neighbors_walk_the_building_and_stop_at_its_ends() {
        let mut p = building(&[3.0, 3.0, 3.0]);
        let b = p.structure.buildings[0].id;
        assert!(p.structure.reorder_floor(b, 0, 2), "zone 0 becomes the top floor, zone 1 the bottom");
        assert_eq!(LevelStack::neighbor(&p, 1, 1), Some(2));
        assert_eq!(LevelStack::neighbor(&p, 2, 1), Some(0));
        assert_eq!(LevelStack::neighbor(&p, 0, 1), None, "nothing above the top floor");
        assert_eq!(LevelStack::neighbor(&p, 1, -1), None, "nothing below the bottom floor");
        assert_eq!(LevelStack::neighbor(&p, 0, -1), Some(2));
        assert_eq!(LevelStack::neighbor(&p, 9, 1), None);
        let lone = Project::new("l");
        assert_eq!(LevelStack::neighbor(&lone, 0, 1), None, "not in a building");
    }

    #[test]
    fn the_active_level_follows_the_chosen_zone_even_when_floors_are_reordered() {
        let mut p = building(&[3.0, 3.0, 3.0]);
        let b = p.structure.buildings[0].id;
        assert!(p.structure.reorder_floor(b, 0, 2));
        // Zone 0 is now the top floor.
        let s = LevelStack::build(&p, 0).unwrap();
        assert_eq!(s.active, 2);
        assert_eq!(s.zones[2], p.zones[0].id);
    }

    #[test]
    fn alignment_translation_becomes_the_level_offset_and_rotation_is_flagged() {
        let mut p = building(&[3.0, 3.0]);
        p.zones[1].planning.level.offset = Point::new(1.0, 1.0);
        p.structure.buildings[0].floors[1].alignment = FloorAlignment { dx_m: 2.0, dy_m: -0.5, rotation_rad: 0.0 };
        let s = LevelStack::build(&p, 0).unwrap();
        assert_eq!(s.levels[0].level.offset, Point::new(0.0, 0.0));
        assert_eq!(s.levels[1].level.offset, Point::new(3.0, 0.5), "stored offset plus the alignment translation");
        assert!(s.warnings.is_empty());
        p.structure.buildings[0].floors[1].alignment.rotation_rad = 0.2;
        assert!(LevelStack::build(&p, 0).unwrap().warnings[0].contains("rotated"));
        assert_eq!(p.zones[1].planning.level.offset, Point::new(1.0, 1.0), "stored data is untouched");
    }

    #[test]
    fn a_nonpositive_floor_height_is_an_error_naming_the_floor() {
        let p = building(&[3.0, 0.0]);
        match LevelStack::build(&p, 0) {
            Err(StackError::BadHeight(label)) => assert!(label.starts_with("Floor")),
            other => panic!("{other:?}"),
        }
        let p = building(&[3.0, f64::NAN]);
        assert!(LevelStack::build(&p, 0).is_err());
    }
}
