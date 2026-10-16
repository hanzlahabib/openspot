//! Renders the heatmap pictures used by the user guide (`docs/guide/img/`) from SYNTHETIC data.
//!
//! Nothing here was measured: a floor plan (`samples/sample-house-16x10m.png`), three made-up
//! dual-band access points, and a simple deterministic path-loss model produce readings on a
//! grid of points. The output is byte-for-byte reproducible, so a diff of the PNGs after
//! regeneration shows exactly what a rendering change did.
//!
//! ```sh
//! cargo run -p openspot-export --example guide_images            # writes docs/guide/img/
//! cargo run -p openspot-export --example guide_images -- some/dir
//! ```

use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use openspot_export::{PngStyle, render_png_styled};
use openspot_heatmap::{GradientPreset, Metric, Smoothing};
use openspot_model::{MapImage, ScanPoint, Snapshot, Zone};
use openspot_wlan::{AccessPoint, MacAddress};

const PLAN: &[u8] = include_bytes!("../../../samples/sample-house-16x10m.png");

/// The sample plan is 1600x1000 px for 16x10 m.
const PLAN_PX_PER_M: f64 = 100.0;
/// Pictures are rendered at this fraction of plan size to keep the committed files small.
const OUTPUT_SCALE: f64 = 0.5;
const HEATMAP_ALPHA: u8 = 170;

/// Log-distance exponent for an indoor home with light partitions.
const PATH_LOSS_EXPONENT: f64 = 3.4;
/// Loss added per interior wall crossed, per band; 5 GHz loses more through walls.
const WALL_LOSS_DB: [f64; 2] = [6.0, 10.0];
/// RSSI at one metre, per band (transmit power minus free-space loss at 1 m).
const REF_RSSI_1M_DBM: [f64; 2] = [-44.0, -54.0];
/// Weaker than this is not reported by a scan, so the radio is omitted from the point.
const NOISE_CUTOFF_DBM: f64 = -92.0;
/// Peak size of the deterministic shadowing term, so contours are not perfect circles.
const SHADOWING_DB: f64 = 2.0;
/// The app counts a band or an AP as present at or above this level (`AP_COUNT_MIN_RSSI_DBM`).
const APP_MIN_RSSI_DBM: i32 = -80;
const SSID: &str = "SyntheticHome";
/// Measurement grid in plan pixels. Columns and rows are chosen to stay clear of the walls and
/// to reach close to the outer wall, so the heatmap fills the whole plan.
const GRID_X: [f64; 10] = [80.0, 235.0, 390.0, 545.0, 700.0, 855.0, 1010.0, 1165.0, 1320.0, 1475.0];
const GRID_Y: [f64; 6] = [90.0, 250.0, 420.0, 550.0, 740.0, 910.0];

/// Interior walls of the sample plan as `(x1, y1, x2, y2)` in plan pixels (doorways are gaps).
const WALLS: &[(f64, f64, f64, f64)] = &[
    (600.0, 0.0, 600.0, 300.0),
    (600.0, 390.0, 600.0, 1000.0),
    (1050.0, 0.0, 1050.0, 250.0),
    (1050.0, 340.0, 1050.0, 750.0),
    (1050.0, 840.0, 1050.0, 1000.0),
    (300.0, 600.0, 300.0, 1000.0),
    (0.0, 600.0, 200.0, 600.0),
    (290.0, 600.0, 400.0, 600.0),
    (480.0, 600.0, 800.0, 600.0),
    (890.0, 600.0, 1050.0, 600.0),
    (1050.0, 500.0, 1600.0, 500.0),
];

/// One physical access point with a radio per band.
struct SyntheticAp {
    id: u8,
    x: f64,
    y: f64,
    channel_24: (u32, u32),
    /// `None` for a 2.4 GHz-only unit.
    channel_5: Option<(u32, u32)>,
}

/// APs 1 and 2 share 2.4 GHz channel 1 on purpose, so the interference map has something to
/// show; AP 3 is a 2.4 GHz-only unit, which gives the band map a 2.4-only region.
const APS: [SyntheticAp; 3] = [
    SyntheticAp { id: 1, x: 300.0, y: 300.0, channel_24: (1, 2_412_000), channel_5: Some((36, 5_180_000)) },
    SyntheticAp { id: 2, x: 800.0, y: 500.0, channel_24: (1, 2_412_000), channel_5: Some((44, 5_220_000)) },
    SyntheticAp { id: 3, x: 1350.0, y: 700.0, channel_24: (11, 2_462_000), channel_5: None },
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::args_os()
        .nth(1)
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide/img"), PathBuf::from);
    std::fs::create_dir_all(&out_dir)?;
    for (name, bytes) in render_all()? {
        std::fs::write(out_dir.join(&name), bytes)?;
        println!("wrote {name}");
    }
    Ok(())
}

/// A picture's file name and its PNG bytes.
type Picture = (String, Vec<u8>);

/// Every picture the guide uses.
fn render_all() -> Result<Vec<Picture>, Box<dyn std::error::Error>> {
    let (zone, snapshot) = scene()?;
    let all = Vec::new();
    let (acceptable, critical) = Metric::Signal(Vec::new()).default_thresholds().ok_or("signal has thresholds")?;
    let metrics = [
        ("signal.png", Metric::Signal(all.clone()), GradientPreset::Default),
        ("secondary-signal.png", Metric::SecondarySignal(all.clone()), GradientPreset::Default),
        ("sir.png", Metric::Sir(all.clone()), GradientPreset::Default),
        ("band-coverage.png", Metric::BandCoverage { wanted: all.clone(), min_rssi_dbm: APP_MIN_RSSI_DBM }, GradientPreset::Default),
        (
            "troubleshoot.png",
            Metric::Troubleshoot { base: Box::new(Metric::Signal(all)), acceptable, critical },
            GradientPreset::Default,
        ),
    ];
    metrics
        .into_iter()
        .map(|(name, metric, preset)| {
            let style = PngStyle { preset, range: None, smoothing: Smoothing::Normal, alpha: HEATMAP_ALPHA, show_points: true };
            Ok((name.to_owned(), render_png_styled(&zone, &snapshot, &metric, &style)?))
        })
        .collect()
}

fn scene() -> Result<(Zone, Snapshot), Box<dyn std::error::Error>> {
    let plan = image::load_from_memory(PLAN)?;
    let (w, h) = ((f64::from(plan.width()) * OUTPUT_SCALE).round() as u32, (f64::from(plan.height()) * OUTPUT_SCALE).round() as u32);
    let mut encoded = std::io::Cursor::new(Vec::new());
    plan.resize_exact(w, h, FilterType::Lanczos3).write_to(&mut encoded, image::ImageFormat::Png)?;

    let mut zone = Zone::new("Sample house (synthetic data)");
    zone.map = Some(MapImage { original_name: "sample-house-16x10m.png".into(), data: encoded.into_inner(), width: w, height: h });

    let mut snapshot = Snapshot::new("Synthetic walk-through");
    for py in GRID_Y {
        for px in GRID_X {
            let heard = readings_at(px, py);
            snapshot.points.push(ScanPoint::new(px * OUTPUT_SCALE, py * OUTPUT_SCALE, 0, heard));
        }
    }
    Ok((zone, snapshot))
}

fn readings_at(px: f64, py: f64) -> Vec<AccessPoint> {
    let mut heard = Vec::new();
    for ap in &APS {
        for (band, (channel, khz)) in [Some(ap.channel_24), ap.channel_5].into_iter().enumerate().filter_map(|(b, c)| Some((b, c?))) {
            let rssi = rssi_at(ap, band, px, py);
            if rssi >= NOISE_CUTOFF_DBM {
                let bssid = MacAddress([0x02, 0, 0, 0, ap.id, band as u8 * 0x10 + channel as u8]);
                heard.push(AccessPoint::from_beacon(SSID.into(), bssid, rssi.round() as i32, khz, true, 100, Vec::new()));
            }
        }
    }
    heard
}

fn rssi_at(ap: &SyntheticAp, band: usize, px: f64, py: f64) -> f64 {
    // Never closer than 1 m: the log model is meaningless inside the near field.
    let metres = (ap.x - px).hypot(ap.y - py).max(PLAN_PX_PER_M) / PLAN_PX_PER_M;
    let walls = WALLS.iter().filter(|w| segments_cross((ap.x, ap.y, px, py), **w)).count() as f64;
    // Fixed per-position wobble stands in for multipath without needing a random generator.
    let shadowing = SHADOWING_DB * ((px * 0.013 + py * 0.021 + f64::from(ap.id)).sin());
    REF_RSSI_1M_DBM[band] - 10.0 * PATH_LOSS_EXPONENT * metres.log10() - walls * WALL_LOSS_DB[band] + shadowing
}

/// Proper intersection of two segments (touching endpoints do not count).
fn segments_cross(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    let side = |(x1, y1, x2, y2): (f64, f64, f64, f64), (px, py): (f64, f64)| (x2 - x1) * (py - y1) - (y2 - y1) * (px - x1);
    let (a1, a2, b1, b2) = ((a.0, a.1), (a.2, a.3), (b.0, b.1), (b.2, b.3));
    side(a, b1) * side(a, b2) < 0.0 && side(b, a1) * side(b, a2) < 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_deterministic_and_complete() {
        let first = render_all().unwrap();
        let second = render_all().unwrap();
        assert_eq!(first, second, "the pictures must be reproducible byte for byte");
        let names: Vec<_> = first.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["signal.png", "secondary-signal.png", "sir.png", "band-coverage.png", "troubleshoot.png"]);
        for (name, bytes) in &first {
            let img = image::load_from_memory(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((img.width(), img.height()), (800, 500), "{name}");
        }
    }

    #[test]
    fn walls_are_counted_along_the_path_and_signal_decays_with_distance() {
        let ap = &APS[0];
        let crossed = |px: f64, py: f64| WALLS.iter().filter(|w| segments_cross((ap.x, ap.y, px, py), **w)).count();
        assert_eq!(crossed(500.0, 200.0), 0);
        assert_eq!(crossed(700.0, 200.0), 1, "the x=600 partition lies between these points");
        assert!(rssi_at(ap, 0, 400.0, 250.0) > rssi_at(ap, 0, 550.0, 250.0));
    }

    #[test]
    fn crossing_detection() {
        assert!(segments_cross((0.0, 0.0, 10.0, 10.0), (0.0, 10.0, 10.0, 0.0)));
        assert!(!segments_cross((0.0, 0.0, 4.0, 4.0), (0.0, 10.0, 10.0, 0.0)));
    }
}
