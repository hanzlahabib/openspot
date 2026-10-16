//! Shared synthetic project for tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use image::{ImageFormat, Rgba, RgbaImage};
use openspot_model::{MapImage, Project, ScanPoint, Snapshot, Uuid, Zone};
use openspot_wlan::{AccessPoint, MacAddress};

use crate::branding::ReportBranding;

/// 2026-10-16 12:00:00 UTC
pub const NOW: i64 = 1_792_152_000;
/// 2026-10-01 09:00:00 UTC and 2026-10-03 10:00:00 UTC
pub const T1: i64 = 1_790_845_200;
pub const T2: i64 = 1_791_021_600;

pub const LOBBY: MacAddress = MacAddress([0xB8, 0x27, 0xEB, 0, 0, 1]);
pub const CAFE: MacAddress = MacAddress([0xF4, 0xF2, 0x6D, 0, 0, 2]);
pub const CJK: MacAddress = MacAddress([0xF4, 0xF2, 0x6D, 0, 0, 3]);
pub const RARE: MacAddress = MacAddress([0xF4, 0xF2, 0x6D, 0, 0, 4]);

pub fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    RgbaImage::from_pixel(w, h, Rgba(color)).write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

pub fn plan_map() -> MapImage {
    MapImage { original_name: "plan.png".into(), data: png(400, 300, [225, 215, 190, 255]), width: 400, height: 300 }
}

pub fn ap(bssid: MacAddress, ssid: &str, rssi: i32, khz: u32) -> AccessPoint {
    AccessPoint::from_beacon(ssid.into(), bssid, rssi, khz, true, 100, vec![])
}

/// Five points over the middle of the plan; LOBBY and CAFE heard everywhere, CJK at
/// the first two points, RARE at one.
pub fn snapshot(name: &str, offset: i32) -> Snapshot {
    let mut s = Snapshot::new(name);
    let coords = [(120.0, 100.0), (280.0, 100.0), (200.0, 150.0), (120.0, 200.0), (280.0, 200.0)];
    for (i, (x, y)) in coords.into_iter().enumerate() {
        let mut aps = vec![ap(LOBBY, "Caf\u{e9} Z\u{fc}rich", -45 - 6 * i as i32 + offset, 2_437_000), ap(CAFE, "Guest", -60 - 3 * i as i32 + offset, 5_180_000)];
        if i < 2 {
            aps.push(ap(CJK, "\u{65e5}\u{672c}\u{8a9e}", -70 + offset, 2_412_000));
        }
        if i == 4 {
            aps.push(ap(RARE, "Printer", -80, 2_462_000));
        }
        s.points.push(ScanPoint::new(x, y, T1 + i as i64 * 60 + if name == "After" { T2 - T1 } else { 0 }, aps));
    }
    s
}

pub fn project() -> Project {
    let mut p = Project::new("HQ Survey");
    p.zones[0].name = "Ground floor".into();
    p.zones[0].map = Some(plan_map());
    p.zones[0].snapshots = vec![snapshot("Before", 0), snapshot("After", 4)];
    let mut upper = Zone::new("Upper floor");
    upper.map = Some(plan_map());
    upper.snapshots = vec![snapshot("Only", -3)];
    let mut roof = Zone::new("Roof (no plan)");
    roof.snapshots = vec![snapshot("Only", -10)];
    p.zones.extend([upper, roof]);
    p
}

pub fn aliases() -> BTreeMap<MacAddress, String> {
    BTreeMap::from([(LOBBY, "Lobby".to_owned())])
}

pub fn branding() -> ReportBranding {
    ReportBranding {
        company: "Acme Networks".into(),
        logo_path: String::new(),
        client: "Contoso Ltd".into(),
        location: "12 Example Street, Z\u{fc}rich".into(),
        contact: "survey@acme.example".into(),
    }
}


/// A scratch directory removed on drop.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("openspot-report-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a leftover temp directory must not fail the test run.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
