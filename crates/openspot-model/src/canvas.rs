//! Per-zone and per-snapshot survey-canvas data (paths, sampling settings and similar).
//!
//! Kept beside the core hierarchy, keyed by the owning zone/snapshot id, so the core
//! `Zone`/`Snapshot` structs stay stable while canvas features evolve.

use std::collections::{BTreeMap, BTreeSet};

use openspot_wlan::MacAddress;

use crate::{CropArea, Drawing, Provenance, Uuid};

/// A planned walking route over the floor plan, in map pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyPath {
    pub waypoints: Vec<(f64, f64)>,
    /// Distance between automatic samples, in meters.
    pub interval_m: f64,
}

/// Default distance between automatic samples: dense enough to see a wall's effect,
/// sparse enough to walk a floor in a few minutes.
pub const DEFAULT_PATH_INTERVAL_M: f64 = 3.0;

impl Default for SurveyPath {
    fn default() -> Self {
        Self { waypoints: Vec::new(), interval_m: DEFAULT_PATH_INTERVAL_M }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapshotCanvas {
    pub path: SurveyPath,
    /// Hidden (empty-SSID) BSSIDs left out of the heatmap and exports. Hidden networks
    /// are included unless listed here.
    pub hidden_excluded: BTreeSet<MacAddress>,
    /// User-chosen names for hidden BSSIDs.
    pub hidden_labels: BTreeMap<MacAddress, String>,
    /// Access-point positions set by hand (map pixels); they override the automatic estimate.
    pub manual_aps: BTreeMap<MacAddress, (f64, f64)>,
    /// Source snapshots when this one was made by merging; empty otherwise.
    pub provenance: Vec<Provenance>,
}

impl SnapshotCanvas {
    /// Whether saving this entry would store anything beyond the defaults.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// How far a measurement point's value is trusted to extend over the plan, named by the
/// kind of environment where that distance is realistic (walls and furniture absorb the
/// signal quickly indoors; it carries much farther in the open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadiusPreset {
    /// Cubicles, small rooms with many partitions.
    Tiny,
    /// Apartments and offices with many walls.
    Small,
    /// Open-plan offices, classrooms, shops.
    Medium,
    /// Large halls, lobbies, warehouses with racking.
    Large,
    /// Stadiums, car parks, open outdoor areas.
    Extra,
}

impl RadiusPreset {
    pub const ALL: [Self; 5] = [Self::Tiny, Self::Small, Self::Medium, Self::Large, Self::Extra];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tiny => "Tiny (cubicles, small rooms)",
            Self::Small => "Small (apartments, walled offices)",
            Self::Medium => "Medium (open-plan offices, shops)",
            Self::Large => "Large (halls, warehouses)",
            Self::Extra => "Extra (stadiums, outdoors)",
        }
    }

    /// Influence radius in meters.
    pub fn meters(self) -> f64 {
        match self {
            Self::Tiny => 1.5,
            Self::Small => 3.0,
            Self::Medium => 6.0,
            Self::Large => 12.0,
            Self::Extra => 25.0,
        }
    }

    /// Stable code for storage; never renumber.
    pub fn code(self) -> i64 {
        Self::ALL.iter().position(|p| *p == self).map_or(0, |i| i as i64)
    }

    pub fn from_code(code: i64) -> Option<Self> {
        usize::try_from(code).ok().and_then(|i| Self::ALL.get(i).copied())
    }
}

/// Most scans averaged into one point. Each takes seconds, and the person has to stand
/// still throughout; beyond this the gain is negligible.
pub const MAX_SAMPLES_PER_POINT: u8 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct ZoneCanvas {
    /// `None` keeps the automatic radius (a fixed fraction of the plan size).
    pub radius: Option<RadiusPreset>,
    /// Scans averaged into each measurement point (1..=[`MAX_SAMPLES_PER_POINT`]).
    pub samples_per_point: u8,
    /// Where the heatmap is drawn and exported; `None` means the whole plan.
    pub crop: Option<CropArea>,
    /// Tint parts of the plan with no measurement within the data point radius gray.
    pub gray_unmeasured: bool,
    /// Plan drawn in the app; when set, the zone's map image is rendered from it.
    pub drawing: Option<Drawing>,
}

impl Default for ZoneCanvas {
    fn default() -> Self {
        Self { radius: None, samples_per_point: 1, crop: None, gray_unmeasured: false, drawing: None }
    }
}

impl ZoneCanvas {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// All canvas data of a project.
#[derive(Debug, Clone, Default)]
pub struct CanvasData {
    zones: BTreeMap<Uuid, ZoneCanvas>,
    snapshots: BTreeMap<Uuid, SnapshotCanvas>,
}

impl CanvasData {
    pub fn zone(&self, id: Uuid) -> Option<&ZoneCanvas> {
        self.zones.get(&id)
    }

    pub fn zone_mut(&mut self, id: Uuid) -> &mut ZoneCanvas {
        self.zones.entry(id).or_default()
    }

    pub fn snapshot(&self, id: Uuid) -> Option<&SnapshotCanvas> {
        self.snapshots.get(&id)
    }

    pub fn snapshot_mut(&mut self, id: Uuid) -> &mut SnapshotCanvas {
        self.snapshots.entry(id).or_default()
    }

    pub fn set_snapshot(&mut self, id: Uuid, data: SnapshotCanvas) {
        self.snapshots.insert(id, data);
    }

    pub fn set_zone(&mut self, id: Uuid, data: ZoneCanvas) {
        self.zones.insert(id, data);
    }

    pub fn remove_zone(&mut self, id: Uuid) {
        self.zones.remove(&id);
    }

    pub fn remove_snapshot(&mut self, id: Uuid) {
        self.snapshots.remove(&id);
    }

    /// Entries that differ from the defaults (what needs persisting).
    pub fn zones(&self) -> impl Iterator<Item = (Uuid, &ZoneCanvas)> {
        self.zones.iter().filter(|(_, z)| !z.is_default()).map(|(id, z)| (*id, z))
    }

    pub fn snapshots(&self) -> impl Iterator<Item = (Uuid, &SnapshotCanvas)> {
        self.snapshots.iter().filter(|(_, s)| !s.is_default()).map(|(id, s)| (*id, s))
    }
}

/// Equality ignores entries that hold only defaults, so a project that touched and reset a
/// setting equals one reloaded from disk (where default entries are not stored).
impl PartialEq for CanvasData {
    fn eq(&self, other: &Self) -> bool {
        self.zones().eq(other.zones()) && self.snapshots().eq(other.snapshots())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radius_codes_round_trip_and_grow_monotonically() {
        for p in RadiusPreset::ALL {
            assert_eq!(RadiusPreset::from_code(p.code()), Some(p));
        }
        assert_eq!(RadiusPreset::from_code(-1), None);
        assert_eq!(RadiusPreset::from_code(99), None);
        let m: Vec<f64> = RadiusPreset::ALL.iter().map(|p| p.meters()).collect();
        assert!(m.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn default_entries_do_not_affect_equality() {
        let mut a = CanvasData::default();
        let id = Uuid::new_v4();
        a.snapshot_mut(id);
        assert_eq!(a, CanvasData::default());
        a.snapshot_mut(id).path.waypoints.push((1.0, 2.0));
        assert_ne!(a, CanvasData::default());
        assert_eq!(a.snapshots().count(), 1);
    }
}
