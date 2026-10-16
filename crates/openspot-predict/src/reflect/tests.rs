use openspot_model::{Band, Material, MaterialCatalog, MaterialCategory, PlanningData, Radio, VirtualAp, Wall, WallKind};

use super::*;
use crate::model::fspl_db;

struct Fixture {
    scene: Scene,
    tx: Transmitter,
}

fn fixture(walls: &[(Point, Point, f64, f64)]) -> Fixture {
    let mut d = PlanningData::default();
    for (i, (a, b, loss, refl)) in walls.iter().enumerate() {
        let id = format!("m{i}");
        d.materials.push(Material::custom(&id, &id, MaterialCategory::Wall, *loss, *refl));
        d.walls.push(Wall::new(*a, *b, WallKind::Wall, id));
    }
    let mut ap = VirtualAp::new("ap", Point::new(0.0, 0.0), vec![Radio::new(Band::Ghz5, 36, 20, 20.0)]);
    ap.height_m = 1.0;
    d.access_points.push(ap);
    let levels = [&d];
    let scene = Scene::build(&levels, &MaterialCatalog::builtin().unwrap()).unwrap();
    Fixture { scene, tx: Scene::transmitters(&levels).unwrap().remove(0) }
}

fn rx(x: f64, y: f64, h: f64) -> Position {
    Position { level: 0, x, y, height_m: h }
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

const MODEL: PropagationModel = PropagationModel { exponent: 2.0, offset_db: 0.0 };

fn mw(dbm: f64) -> f64 {
    10f64.powf(dbm / 10.0)
}

#[test]
fn mirror_wall_adds_the_image_path_power_exactly() {
    // Wall along y = 3; TX (0,0), RX (10,0): image (0,6), reflected length sqrt(136), bounce at (5,3).
    let f = fixture(&[(pt(-50.0, 3.0), pt(50.0, 3.0), 10.0, 0.5)]);
    let freq = f.tx.center_frequency_mhz();
    let gain = f.tx.radio.antenna_gain_dbi;
    let direct_dbm = 20.0 + gain - (fspl_db(1.0, freq) + 20.0 * 10f64.log10());
    let reflected_dbm = 20.0 + gain - (fspl_db(1.0, freq) + 20.0 * 136f64.sqrt().log10() + 20.0 * 2f64.log10());
    let got = rssi_dbm_single_bounce(&f.scene, &MODEL, &f.tx, rx(10.0, 0.0, 1.0)).unwrap();
    let expected = 10.0 * (mw(direct_dbm) + mw(reflected_dbm)).log10();
    assert!((got - expected).abs() < 1e-9, "{got} vs {expected}");
    assert!(got > direct_dbm);
}

#[test]
fn height_difference_lengthens_the_reflected_path() {
    let f = fixture(&[(pt(-50.0, 3.0), pt(50.0, 3.0), 10.0, 0.5)]);
    let freq = f.tx.center_frequency_mhz();
    let gain = f.tx.radio.antenna_gain_dbi;
    // Receiver 4 m higher: direct sqrt(100 + 16), reflected sqrt(136 + 16).
    let direct_dbm = 20.0 + gain - (fspl_db(1.0, freq) + 20.0 * 116f64.sqrt().log10());
    let reflected_dbm = 20.0 + gain - (fspl_db(1.0, freq) + 20.0 * 152f64.sqrt().log10() + 20.0 * 2f64.log10());
    let got = rssi_dbm_single_bounce(&f.scene, &MODEL, &f.tx, rx(10.0, 0.0, 5.0)).unwrap();
    assert!((got - 10.0 * (mw(direct_dbm) + mw(reflected_dbm)).log10()).abs() < 1e-9);
}

#[test]
fn no_reflection_when_geometry_or_material_forbids_it() {
    let direct = |f: &Fixture, to: Position| rssi_dbm(&f.scene, &MODEL, &f.tx, to).unwrap();
    // Opposite sides of the wall: no mirror path (the direct path crosses the wall).
    let f = fixture(&[(pt(-50.0, 3.0), pt(50.0, 3.0), 10.0, 0.5)]);
    let to = rx(10.0, 6.0, 1.0);
    assert_eq!(rssi_dbm_single_bounce(&f.scene, &MODEL, &f.tx, to).unwrap(), direct(&f, to));
    // Wall too short: the bounce point (5, 3) is off its end.
    let f = fixture(&[(pt(7.0, 3.0), pt(50.0, 3.0), 10.0, 0.5)]);
    let to = rx(10.0, 0.0, 1.0);
    assert_eq!(rssi_dbm_single_bounce(&f.scene, &MODEL, &f.tx, to).unwrap(), direct(&f, to));
    // Non-reflective wall.
    let f = fixture(&[(pt(-50.0, 3.0), pt(50.0, 3.0), 10.0, 0.0)]);
    assert_eq!(rssi_dbm_single_bounce(&f.scene, &MODEL, &f.tx, to).unwrap(), direct(&f, to));
    // No walls at all.
    let f = fixture(&[]);
    assert_eq!(rssi_dbm_single_bounce(&f.scene, &MODEL, &f.tx, to).unwrap(), direct(&f, to));
}

#[test]
fn walls_on_the_reflected_legs_attenuate_the_bounce() {
    // A 20 dB partition crossing the first leg (x = 2.5 between TX and the bounce at (5, 3)).
    let open = fixture(&[(pt(-50.0, 3.0), pt(50.0, 3.0), 10.0, 0.5)]);
    let blocked = fixture(&[(pt(-50.0, 3.0), pt(50.0, 3.0), 10.0, 0.5), (pt(2.5, 0.5), pt(2.5, 2.0), 20.0, 0.0)]);
    let to = rx(10.0, 0.0, 1.0);
    let a = rssi_dbm_single_bounce(&open.scene, &MODEL, &open.tx, to).unwrap();
    let b = rssi_dbm_single_bounce(&blocked.scene, &MODEL, &blocked.tx, to).unwrap();
    // Leg from (0,0) to (5,3) passes x=2.5 at y=1.5, inside the partition; the direct path (y=0) does not.
    let direct_only = rssi_dbm(&open.scene, &MODEL, &open.tx, to).unwrap();
    let freq = open.tx.center_frequency_mhz();
    let refl_open = 20.0 + open.tx.radio.antenna_gain_dbi - (fspl_db(1.0, freq) + 20.0 * 136f64.sqrt().log10() + 20.0 * 2f64.log10());
    assert!((b - 10.0 * (mw(direct_only) + mw(refl_open - 20.0)).log10()).abs() < 1e-9);
    assert!(b < a);
}

#[test]
fn reflections_never_reduce_the_signal_and_other_levels_are_direct_only() {
    let f = fixture(&[(pt(-50.0, 3.0), pt(50.0, 3.0), 10.0, 0.6), (pt(-50.0, -4.0), pt(50.0, -4.0), 10.0, 0.6)]);
    for (x, y) in [(3.0, 0.0), (10.0, 1.0), (-8.0, 2.0), (0.5, -2.0)] {
        let to = rx(x, y, 1.0);
        assert!(rssi_dbm_single_bounce(&f.scene, &MODEL, &f.tx, to).unwrap() >= rssi_dbm(&f.scene, &MODEL, &f.tx, to).unwrap());
    }
    // Two walls: a corridor has two single bounces.
    let corridor = rssi_dbm_single_bounce(&f.scene, &MODEL, &f.tx, rx(10.0, 0.0, 1.0)).unwrap();
    let one = fixture(&[(pt(-50.0, 3.0), pt(50.0, 3.0), 10.0, 0.6)]);
    assert!(corridor > rssi_dbm_single_bounce(&one.scene, &MODEL, &one.tx, rx(10.0, 0.0, 1.0)).unwrap());
}
