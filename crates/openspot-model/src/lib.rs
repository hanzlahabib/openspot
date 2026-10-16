//! Domain model for survey projects.
//!
//! Hierarchy: [`Project`] -> [`Zone`] (one floor plan + scale) -> [`Snapshot`] (one
//! survey pass) -> [`ScanPoint`] (a position on the map) -> access points seen there.
//! Coordinates are map-image pixels; use [`Scale`] to convert to meters.

#![forbid(unsafe_code)]

mod aliases;
mod canvas;
mod crop;
mod drawing;
mod duplicate;
mod hidden;
mod link;
mod merge;
mod planning;
mod rect_scale;
mod scale;
mod speed;
mod structure;

pub use aliases::ProjectAliases;
pub use planning::*;
pub use canvas::{
    CanvasData, DEFAULT_PATH_INTERVAL_M, MAX_SAMPLES_PER_POINT, RadiusPreset, SnapshotCanvas, SurveyPath, ZoneCanvas,
};
pub use crop::CropArea;
pub use drawing::{Drawing, FontSize, LineThickness, Pt, Shape, ShapeKind};
pub use link::LinkInfo;
pub use hidden::{HiddenSighting, hidden_networks, without_hidden};
pub use merge::{DEFAULT_TIME_BUCKET_SECS, Merged, MergeOptions, Provenance, merge_snapshots};
pub use rect_scale::{ASPECT_WARN_FRACTION, RectScale, scale_from_rectangle};
pub use scale::{LengthUnit, Scale};
pub use speed::SpeedResult;
pub use structure::{
    Building, BuildingNode, DEFAULT_FLOOR_HEIGHT_M, Floor, FloorAlignment, Outline, ProjectSummary, Structure, ZoneNode,
};
pub use uuid::Uuid;

use openspot_wlan::AccessPoint;

/// An encoded floor-plan image (PNG/JPEG bytes) plus its pixel size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapImage {
    pub original_name: String,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// One measurement location and everything seen from it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanPoint {
    pub id: Uuid,
    /// Map pixel position.
    pub x: f64,
    pub y: f64,
    /// Unix time in seconds (UTC).
    pub time: i64,
    pub access_points: Vec<AccessPoint>,
    /// Active-scan results, one per tested network; empty when no active scan ran.
    pub speed: Vec<SpeedResult>,
    /// The adapter's connection when this point was measured; `None` when it was not
    /// connected or the platform cannot say.
    pub link: Option<LinkInfo>,
}

impl ScanPoint {
    pub fn new(x: f64, y: f64, time: i64, access_points: Vec<AccessPoint>) -> Self {
        Self { id: Uuid::new_v4(), x, y, time, access_points, speed: Vec::new(), link: None }
    }

    /// RSSI of one BSSID at this point, if it was seen.
    pub fn rssi_of(&self, bssid: &openspot_wlan::MacAddress) -> Option<i32> {
        self.access_points.iter().find(|a| &a.bssid == bssid).map(|a| a.rssi_dbm)
    }
}

/// One survey pass over a zone. A zone can hold several, to compare equipment
/// placement or configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub id: Uuid,
    pub name: String,
    pub points: Vec<ScanPoint>,
}

impl Snapshot {
    pub fn new(name: impl Into<String>) -> Self {
        Self { id: Uuid::new_v4(), name: name.into(), points: Vec::new() }
    }

    /// Distinct BSSIDs seen anywhere in the snapshot, strongest sighting first,
    /// with the SSID of that sighting.
    pub fn networks(&self) -> Vec<(openspot_wlan::MacAddress, String, i32)> {
        let mut best: std::collections::BTreeMap<_, (String, i32)> = std::collections::BTreeMap::new();
        for ap in self.points.iter().flat_map(|p| &p.access_points) {
            let e = best.entry(ap.bssid).or_insert_with(|| (ap.ssid.clone(), ap.rssi_dbm));
            if ap.rssi_dbm > e.1 {
                *e = (ap.ssid.clone(), ap.rssi_dbm);
            }
        }
        let mut v: Vec<_> = best.into_iter().map(|(b, (s, r))| (b, s, r)).collect();
        v.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        v
    }
}

/// A floor / area with its own floor plan and scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub id: Uuid,
    pub name: String,
    pub map: Option<MapImage>,
    pub scale: Option<Scale>,
    pub snapshots: Vec<Snapshot>,
    /// Walls, virtual APs and level placement for predictive planning.
    pub planning: PlanningData,
}

impl Zone {
    /// New zone with one empty snapshot, ready for measuring.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            map: None,
            scale: None,
            snapshots: vec![Snapshot::new("Snapshot 1")],
            planning: PlanningData::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub zones: Vec<Zone>,
    /// Buildings and file metadata; see [`Structure`].
    pub structure: Structure,
    /// Canvas features' per-zone/per-snapshot data; see [`CanvasData`].
    pub canvas: CanvasData,
    /// Names for access points that are specific to this project (see [`ProjectAliases`]).
    pub aliases: ProjectAliases,
}

impl Project {
    /// New project with one zone.
    pub fn new(name: impl Into<String>) -> Self {
        Self { id: Uuid::new_v4(), name: name.into(), zones: vec![Zone::new("Zone 1")], structure: Structure::default(), canvas: CanvasData::default(), aliases: ProjectAliases::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_wlan::MacAddress;

    fn ap(last: u8, ssid: &str, rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
    }

    #[test]
    fn networks_report_strongest_sighting() {
        let mut s = Snapshot::new("s");
        s.points.push(ScanPoint::new(0.0, 0.0, 0, vec![ap(1, "a", -70), ap(2, "b", -50)]));
        s.points.push(ScanPoint::new(1.0, 1.0, 1, vec![ap(1, "a", -55)]));
        let n = s.networks();
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].0, n[0].2), (MacAddress([0, 0, 0, 0, 0, 2]), -50));
        assert_eq!(n[1].2, -55);
    }

    #[test]
    fn rssi_of_missing_ap_is_none() {
        let p = ScanPoint::new(0.0, 0.0, 0, vec![ap(1, "a", -60)]);
        assert_eq!(p.rssi_of(&MacAddress([0, 0, 0, 0, 0, 1])), Some(-60));
        assert_eq!(p.rssi_of(&MacAddress([9; 6])), None);
    }

    #[test]
    fn new_project_is_ready_to_measure() {
        let p = Project::new("home");
        assert_eq!(p.zones.len(), 1);
        assert_eq!(p.zones[0].snapshots.len(), 1);
    }
}
