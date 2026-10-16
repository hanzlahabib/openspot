//! Calibrating the propagation model to a survey (P-07): turns a Survey snapshot into the
//! engine's measurements, fits, and reports the fit against the model in use.
//!
//! A survey point says "the best of these networks measured X dBm here". The planned
//! counterpart is the strongest planned radio of the same band at that position, so each point
//! becomes one measurement against that radio.

use std::collections::BTreeSet;

use openspot_model::{Band, MaterialCatalog, PlanningData, Project, Scale, Snapshot};
use openspot_predict::{Calibration, CalibrationError, Measurement, Position, PredictError, PropagationModel, Scene, Transmitter, calibrate, rssi_dbm};
use openspot_wlan::MacAddress;

use super::levels::{LevelStack, StackError};

/// Why a calibration could not be run; each message says what to do.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum FitError {
    #[error("set the plan's scale first: survey points are positioned in pixels")]
    NoScale,
    #[error("this zone has no such snapshot")]
    NoSnapshot,
    #[error("place at least one access point (with a radio switched on) to compare the survey against")]
    NoTransmitters,
    #[error("only {used} survey point(s) can be compared ({skipped}); calibration needs at least {min}", min = openspot_predict::MIN_POINTS)]
    TooFewPoints { used: usize, skipped: String },
    #[error(transparent)]
    Stack(#[from] StackError),
    #[error(transparent)]
    Predict(#[from] PredictError),
    #[error(transparent)]
    Calibration(#[from] CalibrationError),
}

/// Points that could not be used, by reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Skipped {
    /// None of the chosen networks was heard at the point.
    pub no_signal: usize,
    /// The strongest chosen network's band is unknown (no beacon frequency).
    pub unknown_band: usize,
    /// No planned radio in the strongest chosen network's band.
    pub no_radio_in_band: usize,
}

impl Skipped {
    pub fn describe(self) -> String {
        let parts: Vec<String> = [(self.no_signal, "no chosen network heard"), (self.unknown_band, "unknown band"), (self.no_radio_in_band, "no planned radio in that band")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, why)| format!("{n} with {why}"))
            .collect();
        if parts.is_empty() { "none skipped".to_owned() } else { format!("skipped: {}", parts.join(", ")) }
    }
}

/// Measurements built from a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementSet {
    pub measurements: Vec<Measurement>,
    pub skipped: Skipped,
}

fn model_band(b: openspot_wlan::Band) -> Band {
    match b {
        openspot_wlan::Band::Ghz2_4 => Band::Ghz2_4,
        openspot_wlan::Band::Ghz5 => Band::Ghz5,
        openspot_wlan::Band::Ghz6 => Band::Ghz6,
    }
}

/// One measurement per usable survey point. `networks` are the BSSIDs that count as "the
/// planned system" (empty: every network heard). A point's measured value is the strongest
/// of them; it is compared with the planned radio of the same band that the current `model`
/// predicts to be strongest there.
#[allow(clippy::too_many_arguments)]
pub fn measurements_from_snapshot(
    snapshot: &Snapshot,
    networks: &BTreeSet<MacAddress>,
    scale: Scale,
    level: usize,
    rx_height_m: f64,
    scene: &Scene,
    transmitters: &[Transmitter],
    model: &PropagationModel,
) -> Result<MeasurementSet, PredictError> {
    let mut set = MeasurementSet { measurements: Vec::new(), skipped: Skipped::default() };
    for point in &snapshot.points {
        let best = point.access_points.iter().filter(|a| networks.is_empty() || networks.contains(&a.bssid)).max_by_key(|a| a.rssi_dbm);
        let Some(best) = best else {
            set.skipped.no_signal += 1;
            continue;
        };
        let Some(band) = best.band.map(model_band) else {
            set.skipped.unknown_band += 1;
            continue;
        };
        let position = Position { level, x: scale.to_meters(point.x), y: scale.to_meters(point.y), height_m: rx_height_m };
        let mut serving: Option<(usize, f64)> = None;
        for (index, tx) in transmitters.iter().enumerate().filter(|(_, t)| t.radio.band == band) {
            let predicted = rssi_dbm(scene, model, tx, position)?;
            if serving.is_none_or(|(_, p)| predicted > p) {
                serving = Some((index, predicted));
            }
        }
        match serving {
            Some((transmitter, _)) => set.measurements.push(Measurement { transmitter, position, rssi_dbm: f64::from(best.rssi_dbm) }),
            None => set.skipped.no_radio_in_band += 1,
        }
    }
    Ok(set)
}

/// Root-mean-square of predicted minus measured, in dB.
pub fn rmse_db(scene: &Scene, transmitters: &[Transmitter], measurements: &[Measurement], model: &PropagationModel) -> Result<f64, PredictError> {
    if measurements.is_empty() {
        return Ok(0.0);
    }
    let mut sum = 0.0;
    for m in measurements {
        let predicted = rssi_dbm(scene, model, &transmitters[m.transmitter], m.position)?;
        sum += (predicted - m.rssi_dbm).powi(2);
    }
    Ok((sum / measurements.len() as f64).sqrt())
}

/// A fit and how it compares with the model that was in use.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub fit: Calibration,
    pub before_model: PropagationModel,
    pub before_rmse_db: f64,
    pub after_rmse_db: f64,
    pub skipped: Skipped,
}

/// Fits the model for `zone_index` against snapshot `snapshot_index` of that zone.
pub fn fit(project: &Project, zone_index: usize, snapshot_index: usize, networks: &BTreeSet<MacAddress>, catalog: &MaterialCatalog, current: &PropagationModel, rx_height_m: f64) -> Result<Report, FitError> {
    let zone = project.zones.get(zone_index).ok_or(StackError::NoZone)?;
    let scale = zone.scale.ok_or(FitError::NoScale)?;
    let snapshot = zone.snapshots.get(snapshot_index).ok_or(FitError::NoSnapshot)?;
    let stack = LevelStack::build(project, zone_index)?;
    let refs: Vec<&PlanningData> = stack.levels.iter().collect();
    let scene = Scene::build(&refs, catalog)?;
    let transmitters = Scene::transmitters(&refs)?;
    if transmitters.is_empty() {
        return Err(FitError::NoTransmitters);
    }
    let set = measurements_from_snapshot(snapshot, networks, scale, stack.active, rx_height_m, &scene, &transmitters, current)?;
    if set.measurements.len() < openspot_predict::MIN_POINTS {
        return Err(FitError::TooFewPoints { used: set.measurements.len(), skipped: set.skipped.describe() });
    }
    let fit = calibrate(&scene, &transmitters, &set.measurements)?;
    Ok(Report {
        before_rmse_db: rmse_db(&scene, &transmitters, &set.measurements, current)?,
        after_rmse_db: rmse_db(&scene, &transmitters, &set.measurements, &fit.model())?,
        before_model: *current,
        fit,
        skipped: set.skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::predict_input::tests::two_room_project;
    use openspot_model::ScanPoint;
    use openspot_wlan::AccessPoint;

    const TRUE_MODEL: PropagationModel = PropagationModel { exponent: 3.3, offset_db: 5.0 };

    fn mac(n: u8) -> MacAddress {
        MacAddress([0, 0, 0, 0, 0, n])
    }

    fn beacon(n: u8, rssi: i32, khz: u32) -> AccessPoint {
        AccessPoint::from_beacon("net".into(), mac(n), rssi, khz, true, 100, vec![])
    }

    fn catalog() -> MaterialCatalog {
        MaterialCatalog::builtin().unwrap()
    }

    /// Engine inputs for the two-room scene (AP at (5, 5) m, 5 GHz ch 42).
    fn world() -> (Project, Scene, Vec<Transmitter>) {
        let p = two_room_project("concrete-wall");
        let stack = LevelStack::build(&p, 0).unwrap();
        let refs: Vec<&PlanningData> = stack.levels.iter().collect();
        let scene = Scene::build(&refs, &catalog()).unwrap();
        let tx = Scene::transmitters(&refs).unwrap();
        (p, scene, tx)
    }

    /// Survey points (in pixels) at a spread of ranges and both sides of the wall, with the
    /// RSSI the engine predicts under `TRUE_MODEL`, rounded to whole dBm like a real scan.
    fn synthetic_snapshot(p: &Project, scene: &Scene, tx: &[Transmitter]) -> Snapshot {
        let mut snap = Snapshot::new("walk");
        let scale = p.zones[0].scale.unwrap();
        for (x, y) in [(6.0, 5.0), (7.0, 8.0), (8.5, 2.0), (3.0, 1.0), (12.0, 5.0), (14.0, 8.0), (17.0, 3.0), (19.0, 9.0)] {
            let pos = Position { level: 0, x, y, height_m: 1.0 };
            let truth = rssi_dbm(scene, &TRUE_MODEL, &tx[0], pos).unwrap();
            snap.points.push(ScanPoint::new(scale.to_pixels(x), scale.to_pixels(y), 0, vec![beacon(1, truth.round() as i32, 5_210_000)]));
        }
        snap
    }

    #[test]
    fn a_survey_generated_from_a_known_model_recovers_it() {
        let (mut p, scene, tx) = world();
        let snap = synthetic_snapshot(&p, &scene, &tx);
        p.zones[0].snapshots = vec![snap];
        let report = fit(&p, 0, 0, &BTreeSet::new(), &catalog(), &PropagationModel::default(), 1.0).unwrap();
        // Whole-dBm rounding is the only noise, so the fit is close but not exact.
        assert!((report.fit.exponent - TRUE_MODEL.exponent).abs() < 0.1, "exponent {}", report.fit.exponent);
        assert!((report.fit.offset_db - TRUE_MODEL.offset_db).abs() < 1.0, "offset {}", report.fit.offset_db);
        assert_eq!(report.fit.points, 8);
        assert!(report.after_rmse_db < 0.6, "rounding noise only: {}", report.after_rmse_db);
        assert!(report.before_rmse_db > 3.0, "the default model is clearly off: {}", report.before_rmse_db);
        assert!(report.after_rmse_db < report.before_rmse_db / 4.0);
        assert_eq!(report.skipped, Skipped::default());
        assert_eq!(report.before_model, PropagationModel::default());
    }

    #[test]
    fn after_rmse_recomputed_from_scratch_equals_the_engines_rmse() {
        let (mut p, scene, tx) = world();
        p.zones[0].snapshots = vec![synthetic_snapshot(&p, &scene, &tx)];
        let report = fit(&p, 0, 0, &BTreeSet::new(), &catalog(), &PropagationModel::default(), 1.0).unwrap();
        assert!((report.after_rmse_db - report.fit.rmse_db).abs() < 1e-9, "{} vs {}", report.after_rmse_db, report.fit.rmse_db);
        // "Before" is the plain RMSE of the default model over the same points.
        let set = measurements_from_snapshot(&p.zones[0].snapshots[0], &BTreeSet::new(), p.zones[0].scale.unwrap(), 0, 1.0, &scene, &tx, &PropagationModel::default()).unwrap();
        let by_hand = (set.measurements.iter().map(|m| (rssi_dbm(&scene, &PropagationModel::default(), &tx[0], m.position).unwrap() - m.rssi_dbm).powi(2)).sum::<f64>() / set.measurements.len() as f64).sqrt();
        assert!((report.before_rmse_db - by_hand).abs() < 1e-12);
    }

    #[test]
    fn measurements_use_meters_and_the_strongest_chosen_network() {
        let (p, scene, tx) = world();
        let scale = p.zones[0].scale.unwrap();
        let mut snap = Snapshot::new("s");
        // Network 1 loud, network 2 quiet at the same spot.
        snap.points.push(ScanPoint::new(scale.to_pixels(6.0), scale.to_pixels(5.0), 0, vec![beacon(1, -40, 5_210_000), beacon(2, -70, 5_210_000)]));
        let all = measurements_from_snapshot(&snap, &BTreeSet::new(), scale, 0, 1.0, &scene, &tx, &PropagationModel::default()).unwrap();
        assert_eq!(all.measurements.len(), 1);
        let m = all.measurements[0];
        assert_eq!((m.position.x, m.position.y, m.position.height_m, m.position.level), (6.0, 5.0, 1.0, 0));
        assert_eq!((m.transmitter, m.rssi_dbm), (0, -40.0), "best of all networks");
        let only2 = measurements_from_snapshot(&snap, &BTreeSet::from([mac(2)]), scale, 0, 1.0, &scene, &tx, &PropagationModel::default()).unwrap();
        assert_eq!(only2.measurements[0].rssi_dbm, -70.0, "best of the selected networks only");
    }

    #[test]
    fn unusable_points_are_counted_by_reason() {
        let (p, scene, tx) = world();
        let scale = p.zones[0].scale.unwrap();
        let mut snap = Snapshot::new("s");
        let at = |x: f64, aps: Vec<AccessPoint>| ScanPoint::new(scale.to_pixels(x), scale.to_pixels(5.0), 0, aps);
        snap.points.push(at(6.0, vec![beacon(1, -50, 5_210_000)]));
        snap.points.push(at(7.0, vec![]));
        snap.points.push(at(8.0, vec![beacon(9, -50, 5_210_000)]));
        snap.points.push(at(9.0, vec![beacon(1, -50, 2_437_000)]));
        snap.points.push(at(9.5, vec![beacon(1, -50, 1_000)]));
        let set = measurements_from_snapshot(&snap, &BTreeSet::from([mac(1)]), scale, 0, 1.0, &scene, &tx, &PropagationModel::default()).unwrap();
        assert_eq!(set.measurements.len(), 1);
        assert_eq!(set.skipped, Skipped { no_signal: 2, unknown_band: 1, no_radio_in_band: 1 });
        assert_eq!(set.skipped.describe(), "skipped: 2 with no chosen network heard, 1 with unknown band, 1 with no planned radio in that band");
        assert_eq!(Skipped::default().describe(), "none skipped");
    }

    #[test]
    fn each_point_is_compared_with_the_radio_predicted_to_be_loudest_in_its_band() {
        let mut p = two_room_project("drywall");
        // A second AP in the far room: points there should be matched with it, not with AP 1.
        let mut second = p.zones[0].planning.access_points[0].clone();
        second.id = openspot_model::Uuid::new_v4();
        second.name = "AP 2".into();
        second.position = openspot_model::Point::new(15.0, 5.0);
        p.zones[0].planning.access_points.push(second);
        let stack = LevelStack::build(&p, 0).unwrap();
        let refs: Vec<&PlanningData> = stack.levels.iter().collect();
        let scene = Scene::build(&refs, &catalog()).unwrap();
        let tx = Scene::transmitters(&refs).unwrap();
        let scale = p.zones[0].scale.unwrap();
        let mut snap = Snapshot::new("s");
        for x in [4.0, 16.0] {
            snap.points.push(ScanPoint::new(scale.to_pixels(x), scale.to_pixels(5.0), 0, vec![beacon(1, -55, 5_210_000)]));
        }
        let set = measurements_from_snapshot(&snap, &BTreeSet::new(), scale, 0, 1.0, &scene, &tx, &PropagationModel::default()).unwrap();
        assert_eq!(set.measurements.iter().map(|m| m.transmitter).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn fewer_than_three_usable_points_is_refused_with_the_reasons() {
        let (mut p, scene, tx) = world();
        let mut snap = synthetic_snapshot(&p, &scene, &tx);
        snap.points.truncate(2);
        p.zones[0].snapshots = vec![snap];
        let err = fit(&p, 0, 0, &BTreeSet::new(), &catalog(), &PropagationModel::default(), 1.0).unwrap_err();
        assert!(matches!(err, FitError::TooFewPoints { used: 2, .. }), "{err:?}");
        assert!(err.to_string().contains("at least 3"));
        // Three points whose networks are not selected are not usable either.
        p.zones[0].snapshots[0] = synthetic_snapshot(&p, &scene, &tx);
        let err = fit(&p, 0, 0, &BTreeSet::from([mac(7)]), &catalog(), &PropagationModel::default(), 1.0).unwrap_err();
        assert!(matches!(&err, FitError::TooFewPoints { used: 0, skipped } if skipped.contains("8 with no chosen network heard")), "{err:?}");
    }

    #[test]
    fn setup_problems_are_explained() {
        let (mut p, scene, tx) = world();
        p.zones[0].snapshots = vec![synthetic_snapshot(&p, &scene, &tx)];
        let run = |p: &Project, snap: usize| fit(p, 0, snap, &BTreeSet::new(), &catalog(), &PropagationModel::default(), 1.0);
        assert_eq!(run(&p, 3).unwrap_err(), FitError::NoSnapshot);
        let mut no_ap = p.clone();
        no_ap.zones[0].planning.access_points.clear();
        assert_eq!(run(&no_ap, 0).unwrap_err(), FitError::NoTransmitters);
        let mut no_scale = p.clone();
        no_scale.zones[0].scale = None;
        assert_eq!(run(&no_scale, 0).unwrap_err(), FitError::NoScale);
        let mut bad = p.clone();
        bad.zones[0].planning.walls[0].material = "gone".into();
        assert!(matches!(run(&bad, 0).unwrap_err(), FitError::Predict(PredictError::UnknownMaterial(_))));
    }

    #[test]
    fn all_points_at_one_range_cannot_fix_an_exponent() {
        let (mut p, scene, tx) = world();
        let scale = p.zones[0].scale.unwrap();
        let mut snap = Snapshot::new("ring");
        for (x, y) in [(6.0, 5.0), (5.0, 6.0), (4.0, 5.0), (5.0, 4.0)] {
            let pos = Position { level: 0, x, y, height_m: 1.0 };
            snap.points.push(ScanPoint::new(scale.to_pixels(x), scale.to_pixels(y), 0, vec![beacon(1, rssi_dbm(&scene, &TRUE_MODEL, &tx[0], pos).unwrap().round() as i32, 5_210_000)]));
        }
        p.zones[0].snapshots = vec![snap];
        let err = fit(&p, 0, 0, &BTreeSet::new(), &catalog(), &PropagationModel::default(), 1.0).unwrap_err();
        assert_eq!(err, FitError::Calibration(CalibrationError::DegenerateGeometry));
    }
}
