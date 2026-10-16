//! Golden-image tests: tiny deterministic heatmap exports compared pixel for pixel against
//! reference PNGs in `tests/golden/`. Regenerate after an intentional change with
//! `UPDATE_GOLDEN=1 cargo test -p openspot-export --test golden` and review the diff of the
//! images (see docs/TESTING.md).

use std::path::{Path, PathBuf};

use image::RgbaImage;
use openspot_export::{PngStyle, render_png_styled};
use openspot_heatmap::{GradientPreset, Metric, Smoothing};
use openspot_model::{MapImage, ScanPoint, Snapshot, Zone};
use openspot_wlan::{AccessPoint, MacAddress};

const WIDTH: u32 = 96;
const HEIGHT: u32 = 64;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

/// A floor plan that is not flat, so blending over it is exercised: a light checkerboard
/// with a darker border.
fn plan_png() -> Vec<u8> {
    let img = RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
        if x < 2 || y < 2 || x >= WIDTH - 2 || y >= HEIGHT - 2 {
            image::Rgba([60, 60, 70, 255])
        } else if (x / 8 + y / 8) % 2 == 0 {
            image::Rgba([245, 245, 240, 255])
        } else {
            image::Rgba([225, 228, 232, 255])
        }
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).expect("encoding an in-memory PNG cannot fail");
    out.into_inner()
}

fn ap(last: u8, rssi: i32) -> AccessPoint {
    AccessPoint::from_beacon("net".into(), MacAddress([2, 0, 0, 0, 0, last]), rssi, 5_180_000, true, 100, vec![])
}

fn scene() -> (Zone, Snapshot) {
    let mut zone = Zone::new("golden");
    zone.map = Some(MapImage { original_name: "plan.png".into(), data: plan_png(), width: WIDTH, height: HEIGHT });
    let mut snap = Snapshot::new("golden");
    for (x, y, a, b) in [(10.0, 10.0, -42, -80), (85.0, 12.0, -61, -58), (48.0, 32.0, -70, -66), (12.0, 55.0, -78, -90), (84.0, 54.0, -88, -49)] {
        snap.points.push(ScanPoint::new(x, y, 0, vec![ap(1, a), ap(2, b)]));
    }
    (zone, snap)
}

fn render(metric: &Metric, preset: GradientPreset, range: Option<(f32, f32)>) -> RgbaImage {
    let (zone, snap) = scene();
    let style = PngStyle { preset, range, smoothing: Smoothing::Normal, alpha: 180, show_points: true };
    let bytes = render_png_styled(&zone, &snap, metric, &style).expect("render");
    image::load_from_memory(&bytes).expect("decode").to_rgba8()
}

/// `Ok` when `actual` equals `expected`; otherwise a description with the number of
/// differing pixels and the first one.
fn diff(expected: &RgbaImage, actual: &RgbaImage) -> Result<(), String> {
    if expected.dimensions() != actual.dimensions() {
        return Err(format!("size {:?} != {:?}", expected.dimensions(), actual.dimensions()));
    }
    let mut count = 0usize;
    let mut first = None;
    for (x, y, e) in expected.enumerate_pixels() {
        let a = actual.get_pixel(x, y);
        if a != e {
            count += 1;
            first.get_or_insert((x, y, *e, *a));
        }
    }
    match first {
        None => Ok(()),
        Some((x, y, e, a)) => Err(format!("{count} pixels differ; first at ({x}, {y}): expected {e:?}, got {a:?}")),
    }
}

fn check(name: &str, actual: &RgbaImage) {
    let path = golden_dir().join(format!("{name}.png"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        actual.save(&path).expect("write golden");
        return;
    }
    let expected = image::open(&path)
        .unwrap_or_else(|e| panic!("missing or unreadable golden {}: {e}\nrun with UPDATE_GOLDEN=1 to create it", path.display()))
        .to_rgba8();
    if let Err(why) = diff(&expected, actual) {
        // Keep the failing output outside the source tree for inspection.
        let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.actual.png"));
        let saved = actual.save(&out).map(|()| out.display().to_string()).unwrap_or_else(|e| format!("(could not save: {e})"));
        panic!("golden `{name}` mismatch: {why}\nactual image: {saved}");
    }
}

#[test]
fn signal_map_default_gradient() {
    check("signal_default", &render(&Metric::Signal(vec![]), GradientPreset::Default, None));
}

#[test]
fn single_ap_with_blue_red_preset_and_custom_range() {
    let metric = Metric::Signal(vec![MacAddress([2, 0, 0, 0, 0, 2])]);
    check("ap2_bluered_range", &render(&metric, GradientPreset::BlueRed, Some((-95.0, -45.0))));
}

#[test]
fn troubleshooting_classes() {
    let metric = Metric::Troubleshoot { base: Box::new(Metric::Signal(vec![])), acceptable: -67, critical: -75 };
    check("troubleshoot", &render(&metric, GradientPreset::Default, None));
}

#[test]
fn ap_count_map() {
    check("ap_count", &render(&Metric::ApCount { min_rssi_dbm: -80 }, GradientPreset::RedGreen, None));
}

#[test]
fn the_comparison_detects_a_single_changed_pixel() {
    let a = render(&Metric::Signal(vec![]), GradientPreset::Default, None);
    assert!(diff(&a, &a).is_ok());
    let mut b = a.clone();
    let p = b.get_pixel_mut(5, 7);
    p[0] = p[0].wrapping_add(1);
    let why = diff(&a, &b).unwrap_err();
    assert!(why.contains("1 pixels differ") && why.contains("(5, 7)"), "{why}");
    let small = RgbaImage::new(2, 2);
    assert!(diff(&a, &small).unwrap_err().contains("size"));
}

#[test]
fn different_inputs_give_different_images() {
    let a = render(&Metric::Signal(vec![]), GradientPreset::Default, None);
    let b = render(&Metric::Signal(vec![]), GradientPreset::BlueRed, None);
    let c = render(&Metric::Signal(vec![]), GradientPreset::Default, Some((-95.0, -45.0)));
    assert!(diff(&a, &b).is_err() && diff(&a, &c).is_err(), "goldens would not notice a broken preset or range");
}
