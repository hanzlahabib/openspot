//! Planning (predictive) data attached to a [`crate::Zone`].
//!
//! Geometry is in meters. Pure data: propagation lives in the `openspot-predict` crate.

mod ap;
mod material;
mod structure;

use serde::{Deserialize, Serialize};

pub use ap::{AntennaError, AntennaPattern, AntennaTable, DEFAULT_AP_HEIGHT_M, DEFAULT_FRONT_TO_BACK_DB, Radio, VirtualAp};
pub use material::{
    Band, CatalogError, Material, MaterialCatalog, MaterialCategory, P2040Params, PerBand, SPEED_OF_LIGHT,
    slab_transmission_reflection,
};
pub use structure::{Cutout, Level, Opening, OpeningKind, Point, Slab, Wall, WallKind};

use crate::Zone;

/// Everything the planner stores for one zone (level). `Default` is an empty level.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlanningData {
    pub level: Level,
    pub walls: Vec<Wall>,
    pub cutouts: Vec<Cutout>,
    pub access_points: Vec<VirtualAp>,
    /// User-defined materials of this zone; they shadow catalog presets with the same id.
    pub materials: Vec<Material>,
}

impl PlanningData {
    /// Looks `id` up in this zone's custom materials, then the preset `catalog`.
    pub fn resolve_material<'a>(&'a self, id: &str, catalog: &'a MaterialCatalog) -> Option<&'a Material> {
        self.materials.iter().find(|m| m.id == id).or_else(|| catalog.get(id))
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Sets `level.elevation_m` of each zone from the floor heights below it. `zones` must be
/// ordered bottom to top; the first level sits at `base_elevation_m`.
pub fn stack_elevations(zones: &mut [Zone], base_elevation_m: f64) {
    let mut z = base_elevation_m;
    for zone in zones {
        zone.planning.level.elevation_m = z;
        z += zone.planning.level.floor_height_m;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_and_resolution_prefers_custom() {
        let catalog = MaterialCatalog::builtin().unwrap();
        let mut p = PlanningData::default();
        assert!(p.is_empty());
        let preset = p.resolve_material("drywall", &catalog).unwrap().absorption_db.ghz5;
        p.materials.push(Material::custom("drywall", "Mine", MaterialCategory::Wall, 99.0, 0.5));
        assert_eq!(p.resolve_material("drywall", &catalog).unwrap().absorption_db.ghz5, 99.0);
        assert_ne!(preset, 99.0);
        assert!(p.resolve_material("nope", &catalog).is_none());
        assert!(!p.is_empty());
    }

    #[test]
    fn elevations_stack_by_floor_height() {
        let mut zones = vec![Zone::new("a"), Zone::new("b"), Zone::new("c")];
        zones[0].planning.level.floor_height_m = 4.0;
        stack_elevations(&mut zones, 0.5);
        let e: Vec<f64> = zones.iter().map(|z| z.planning.level.elevation_m).collect();
        assert_eq!(e, vec![0.5, 4.5, 7.5]);
    }
}
