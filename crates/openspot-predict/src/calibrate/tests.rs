use openspot_model::{
    AntennaPattern, Material, MaterialCatalog, MaterialCategory, PlanningData, Point, Radio, VirtualAp, Wall, WallKind,
};

use super::*;
use crate::grid::rssi_dbm;

fn scene_with_wall(with_wall: bool) -> (Scene, Vec<Transmitter>) {
    let mut d = PlanningData::default();
    d.materials.push(Material::custom("w12", "w12", MaterialCategory::Wall, 12.0, 0.5));
    if with_wall {
        d.walls.push(Wall::new(Point::new(15.0, -100.0), Point::new(15.0, 100.0), WallKind::Wall, "w12"));
    }
    d.access_points.push(VirtualAp::new("a", Point::new(0.0, 0.0), vec![Radio::new(Band::Ghz5, 36, 20, 18.0)]));
    d.access_points.push(VirtualAp::new("b", Point::new(-5.0, 30.0), vec![Radio::new(Band::Ghz2_4, 6, 20, 14.0)]));
    let levels = [&d];
    (Scene::build(&levels, &MaterialCatalog::builtin().unwrap()).unwrap(), Scene::transmitters(&levels).unwrap())
}

/// Deterministic noise in [-1, 1): a 64-bit LCG, so the test needs no RNG crate.
fn noise(state: &mut u64) -> f64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    ((*state >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
}

fn synth(scene: &Scene, tx: &[Transmitter], truth: PropagationModel, count: usize, noise_amp: f64) -> Vec<Measurement> {
    let mut s = 12345u64;
    (0..count)
        .map(|i| {
            let t = i % tx.len();
            // Distances ~1.5 m to ~45 m, spread around the AP.
            let r = 1.5 + 43.5 * (i as f64 / count as f64);
            let a = i as f64 * 2.399963;
            let position = Position { level: 0, x: tx[t].position.x + r * a.cos(), y: tx[t].position.y + r * a.sin(), height_m: 1.0 };
            let rssi = rssi_dbm(scene, &truth, &tx[t], position).unwrap() + noise_amp * noise(&mut s);
            Measurement { transmitter: t, position, rssi_dbm: rssi }
        })
        .collect()
}

#[test]
fn recovers_exact_parameters_from_noise_free_data() {
    let (scene, tx) = scene_with_wall(true);
    let truth = PropagationModel { exponent: 3.1, offset_db: 4.0 };
    let m = synth(&scene, &tx, truth, 24, 0.0);
    let c = calibrate(&scene, &tx, &m).unwrap();
    assert!((c.exponent - 3.1).abs() < 1e-9 && (c.offset_db - 4.0).abs() < 1e-9, "{c:?}");
    assert!(c.rmse_db < 1e-9);
    assert_eq!(c.points, 24);
}

#[test]
fn noisy_data_recovers_parameters_within_tolerance_and_reports_rmse() {
    let (scene, tx) = scene_with_wall(true);
    let truth = PropagationModel { exponent: 2.7, offset_db: -3.0 };
    let amp = 3.0; // uniform noise: std = amp / sqrt(3) ~ 1.73 dB
    let m = synth(&scene, &tx, truth, 120, amp);
    let c = calibrate(&scene, &tx, &m).unwrap();
    assert!((c.exponent - 2.7).abs() < 0.2, "n = {}", c.exponent);
    assert!((c.offset_db - -3.0).abs() < 2.0, "offset = {}", c.offset_db);
    let std = amp / 3f64.sqrt();
    assert!((c.rmse_db - std).abs() < 0.3, "rmse {} vs noise std {std}", c.rmse_db);
    // Reported RMSE equals the error of the engine using the fitted model.
    let fitted = c.model();
    let direct = (m.iter().map(|k| (k.rssi_dbm - rssi_dbm(&scene, &fitted, &tx[k.transmitter], k.position).unwrap()).powi(2)).sum::<f64>() / 120.0).sqrt();
    assert!((direct - c.rmse_db).abs() < 1e-9);
    assert_eq!(c.residuals_db.len(), 120);
    assert!(c.residuals_db.iter().sum::<f64>().abs() < 1e-6, "OLS residuals sum to zero");
}

#[test]
fn known_wall_losses_are_removed_before_fitting() {
    let (scene, tx) = scene_with_wall(true);
    let truth = PropagationModel { exponent: 3.0, offset_db: 2.0 };
    let m = synth(&scene, &tx, truth, 60, 0.0);
    // The same readings fitted against a scene that does not know about the wall are biased.
    let (blind, tx_blind) = scene_with_wall(false);
    let blind_fit = calibrate(&blind, &tx_blind, &m).unwrap();
    let right_fit = calibrate(&scene, &tx, &m).unwrap();
    assert!((right_fit.exponent - 3.0).abs() < 1e-9);
    assert!((blind_fit.exponent - 3.0).abs() > 0.1 || blind_fit.rmse_db > 1.0);
}

#[test]
fn directional_antenna_gain_is_accounted_for() {
    let (scene, mut tx) = scene_with_wall(false);
    tx[0].radio.pattern = AntennaPattern::Directional { azimuth_deg: 0.0, h_beamwidth_deg: 60.0, v_beamwidth_deg: 60.0, tilt_deg: 0.0, front_to_back_db: 25.0 };
    let truth = PropagationModel { exponent: 2.2, offset_db: 1.0 };
    let m: Vec<_> = synth(&scene, &tx, truth, 40, 0.0).into_iter().filter(|k| k.transmitter == 0).collect();
    let c = calibrate(&scene, &tx, &m).unwrap();
    assert!((c.exponent - 2.2).abs() < 1e-9 && (c.offset_db - 1.0).abs() < 1e-9, "{c:?}");
}

fn at(tx: usize, x: f64, y: f64, rssi: f64) -> Measurement {
    Measurement { transmitter: tx, position: Position { level: 0, x, y, height_m: 1.0 }, rssi_dbm: rssi }
}

#[test]
fn refuses_too_few_points() {
    let (scene, tx) = scene_with_wall(false);
    assert_eq!(calibrate(&scene, &tx, &[]).unwrap_err(), CalibrationError::TooFewPoints(0));
    assert_eq!(calibrate(&scene, &tx, &[at(0, 3.0, 0.0, -50.0), at(0, 20.0, 0.0, -70.0)]).unwrap_err(), CalibrationError::TooFewPoints(2));
}

#[test]
fn refuses_degenerate_geometry() {
    let (scene, tx) = scene_with_wall(false);
    // A ring: every point 10 m from the AP.
    let ring: Vec<_> = (0..8).map(|i| {
        let a = f64::from(i) * std::f64::consts::FRAC_PI_4;
        at(0, 10.0 * a.cos(), 10.0 * a.sin(), -60.0 - f64::from(i))
    }).collect();
    assert_eq!(calibrate(&scene, &tx, &ring).unwrap_err(), CalibrationError::DegenerateGeometry);
    // Narrow range of distances (10-14 m: < 2:1).
    let narrow = [at(0, 10.0, 0.0, -60.0), at(0, 12.0, 0.0, -62.0), at(0, 14.0, 0.0, -64.0), at(0, 11.0, 1.0, -61.0)];
    assert_eq!(calibrate(&scene, &tx, &narrow).unwrap_err(), CalibrationError::DegenerateGeometry);
    // All inside the 1 m reference distance.
    let near = [at(0, 0.2, 0.0, -40.0), at(0, 0.0, 0.5, -41.0), at(0, 0.7, 0.0, -40.0)];
    assert_eq!(calibrate(&scene, &tx, &near).unwrap_err(), CalibrationError::DegenerateGeometry);
}

#[test]
fn refuses_invalid_measurements_and_implausible_fits() {
    let (scene, tx) = scene_with_wall(false);
    let ok = [at(0, 2.0, 0.0, -50.0), at(0, 8.0, 0.0, -60.0), at(0, 30.0, 0.0, -70.0)];
    assert!(calibrate(&scene, &tx, &ok).is_ok());
    let mut nan = ok;
    nan[1].rssi_dbm = f64::NAN;
    assert_eq!(calibrate(&scene, &tx, &nan).unwrap_err(), CalibrationError::InvalidMeasurement(1));
    let mut bad_tx = ok;
    bad_tx[2].transmitter = 9;
    assert_eq!(calibrate(&scene, &tx, &bad_tx).unwrap_err(), CalibrationError::InvalidMeasurement(2));
    let mut bad_level = ok;
    bad_level[0].position.level = 5;
    assert!(matches!(calibrate(&scene, &tx, &bad_level), Err(CalibrationError::Predict(_))));
    // Signal getting stronger with distance cannot be a path-loss law.
    let rising = [at(0, 2.0, 0.0, -80.0), at(0, 8.0, 0.0, -60.0), at(0, 30.0, 0.0, -40.0)];
    assert!(matches!(calibrate(&scene, &tx, &rising), Err(CalibrationError::ImplausibleExponent(n)) if n < 0.0));
    // Absurdly steep.
    let steep = [at(0, 2.0, 0.0, -40.0), at(0, 8.0, 0.0, -100.0), at(0, 30.0, 0.0, -160.0)];
    assert!(matches!(calibrate(&scene, &tx, &steep), Err(CalibrationError::ImplausibleExponent(n)) if n > MAX_PLAUSIBLE_EXPONENT));
}
