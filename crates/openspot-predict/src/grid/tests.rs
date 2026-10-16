use std::sync::Mutex;

use openspot_model::{AntennaPattern, Material, MaterialCatalog, MaterialCategory, PlanningData, Point, Radio, Slab, VirtualAp, Wall, WallKind};

use super::*;
use crate::model::fspl_db;

fn room(extra_level: bool) -> (Scene, Vec<Transmitter>) {
    let mut l0 = PlanningData::default();
    l0.materials.push(Material::custom("w10", "w10", MaterialCategory::Wall, 10.0, 0.5));
    l0.materials.push(Material::custom("slab", "slab", MaterialCategory::Slab, 20.0, 0.5));
    l0.level.slab = Slab { thickness_m: 0.2, material: "slab".into() };
    let mut ap = VirtualAp::new("ap", Point::new(10.0, 10.0), vec![Radio::new(Band::Ghz5, 36, 20, 20.0)]);
    ap.height_m = 2.0;
    l0.access_points.push(ap);
    let mut l1 = l0.clone();
    l1.access_points.clear();
    l1.level.elevation_m = 3.0;
    let levels: Vec<&PlanningData> = if extra_level { vec![&l0, &l1] } else { vec![&l0] };
    let scene = Scene::build(&levels, &MaterialCatalog::builtin().unwrap()).unwrap();
    (scene, Scene::transmitters(&levels).unwrap())
}

fn request(level: usize) -> GridRequest {
    GridRequest { level, width_m: 20.0, height_m: 20.0, cell_m: 1.0, rx_height_m: 1.0, pixels_per_meter: 10.0, mode: Mode::Direct }
}

fn open_grid() -> (Grid, Transmitter) {
    let (scene, tx) = room(false);
    let g = rssi_grid(&scene, &PropagationModel::default(), &tx[0], &request(0), &Control::default()).unwrap();
    (g, tx[0].clone())
}

#[test]
fn grid_geometry_is_heatmap_compatible() {
    let (g, _) = open_grid();
    assert_eq!((g.cols, g.rows, g.cell_px), (20, 20, 10.0));
    assert!(g.values.iter().all(Option::is_some));
    let (scene, tx) = room(false);
    let req = GridRequest { width_m: 10.0, height_m: 7.0, cell_m: 0.5, pixels_per_meter: 40.0, ..request(0) };
    let g = rssi_grid(&scene, &PropagationModel::default(), &tx[0], &req, &Control::default()).unwrap();
    assert_eq!((g.cols, g.rows, g.cell_px, g.values.len()), (20, 14, 20.0, 280));
}

#[test]
fn cell_value_matches_the_closed_form() {
    let (g, tx) = open_grid();
    // Cell (14, 10): center (14.5, 10.5); AP at (10, 10) h=2, receiver h=1.
    let d = (4.5f64 * 4.5 + 0.5 * 0.5 + 1.0).sqrt();
    let model = PropagationModel::default();
    let f = tx.center_frequency_mhz();
    let expected = 20.0 + tx.radio.antenna_gain_dbi - (fspl_db(1.0, f) + 10.0 * model.exponent * d.log10());
    assert!((f64::from(g.get(14, 10).unwrap()) - expected).abs() < 1e-3, "{expected}");
}

#[test]
fn open_room_is_symmetric_and_monotone_with_distance() {
    let (g, _) = open_grid();
    let v = |c: usize, r: usize| g.get(c, r).unwrap();
    // AP at (10, 10) is the corner shared by cells 9 and 10: mirror about it.
    for (c, r) in [(0, 0), (3, 7), (9, 9), (15, 2), (19, 19)] {
        let (mc, mr) = (19 - c, 19 - r);
        assert!((v(c, r) - v(mc, mr)).abs() < 1e-4);
        assert!((v(c, r) - v(mc, r)).abs() < 1e-4);
        assert!((v(c, r) - v(c, mr)).abs() < 1e-4);
    }
    // Signal never rises moving away from the AP along a row and a diagonal.
    for c in 10..19 {
        assert!(v(c + 1, 10) <= v(c, 10));
        assert!(v(c + 1, c + 1) <= v(c, c));
    }
    // Equal distance -> equal value (x/y transposition).
    assert!((v(4, 12) - v(12, 4)).abs() < 1e-4);
}

#[test]
fn a_wall_subtracts_exactly_its_loss_behind_it() {
    let (_, tx) = room(false);
    let mut l0 = PlanningData::default();
    l0.materials.push(Material::custom("w10", "w10", MaterialCategory::Wall, 10.0, 0.5));
    l0.walls.push(Wall::new(Point::new(14.0, 0.0), Point::new(14.0, 20.0), WallKind::Wall, "w10"));
    let scene = Scene::build(&[&l0], &MaterialCatalog::builtin().unwrap()).unwrap();
    let walled = rssi_grid(&scene, &PropagationModel::default(), &tx[0], &request(0), &Control::default()).unwrap();
    let (open, _) = open_grid();
    for row in [0, 10, 19] {
        for col in 0..14 {
            assert_eq!(walled.get(col, row), open.get(col, row), "in front ({col},{row})");
        }
        for col in 14..20 {
            let diff = open.get(col, row).unwrap() - walled.get(col, row).unwrap();
            assert!((diff - 10.0).abs() < 1e-4, "behind ({col},{row}) lost {diff}");
        }
    }
}

#[test]
fn other_levels_contribute_through_the_slab() {
    let (scene, tx) = room(true);
    let same = rssi_grid(&scene, &PropagationModel::default(), &tx[0], &request(0), &Control::default()).unwrap();
    let above = rssi_grid(&scene, &PropagationModel::default(), &tx[0], &request(1), &Control::default()).unwrap();
    // Cell (10, 10) on level 1 is 3 m above the level-0 cell: z 4 vs AP z 2 -> d = sqrt(0.5^2*2 + 2^2).
    let d = (0.5f64 * 0.5 * 2.0 + 4.0).sqrt();
    let f = tx[0].center_frequency_mhz();
    let expected = 20.0 + tx[0].radio.antenna_gain_dbi - (fspl_db(1.0, f) + 25.0 * d.log10()) - 20.0;
    assert!((f64::from(above.get(10, 10).unwrap()) - expected).abs() < 1e-3);
    assert!(above.get(10, 10) < same.get(10, 10));
}

#[test]
fn directional_antenna_favours_its_boresight() {
    let (scene, mut tx) = room(false);
    tx[0].radio.pattern = AntennaPattern::Directional { azimuth_deg: 0.0, h_beamwidth_deg: 60.0, v_beamwidth_deg: 60.0, tilt_deg: 0.0, front_to_back_db: 25.0 };
    let g = rssi_grid(&scene, &PropagationModel::default(), &tx[0], &request(0), &Control::default()).unwrap();
    let (front, back, side) = (g.get(15, 10).unwrap(), g.get(4, 10).unwrap(), g.get(10, 15).unwrap());
    assert!(front > side && side > back);
    // Mirror cells about the boresight axis (row 9/10 around y = 10) agree.
    assert!((g.get(15, 9).unwrap() - g.get(15, 10).unwrap()).abs() < 1e-4);
}

#[test]
fn parallel_and_repeated_runs_equal_the_serial_reference() {
    let (scene, tx) = room(false);
    let model = PropagationModel::default();
    let req = request(0);
    let reference: Vec<Option<f32>> = (0..20)
        .flat_map(|row| (0..20).map(move |col| (col, row)))
        .map(|(col, row)| Some(rssi_dbm(&scene, &model, &tx[0], req.cell_center(col, row)).unwrap() as f32))
        .collect();
    let runs: Vec<Vec<Option<f32>>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4).map(|_| s.spawn(|| rssi_grid(&scene, &model, &tx[0], &req, &Control::default()).unwrap().values)).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for run in runs {
        assert_eq!(run, reference);
    }
}

#[test]
fn progress_reports_every_row_once() {
    let (scene, tx) = room(false);
    let seen = Mutex::new(Vec::new());
    let report = |p: Progress| seen.lock().unwrap().push(p);
    let control = Control { cancel: None, progress: Some(&report) };
    rssi_grid(&scene, &PropagationModel::default(), &tx[0], &request(0), &control).unwrap();
    let mut seen = seen.into_inner().unwrap();
    seen.sort_by_key(|p| p.done);
    assert_eq!(seen.iter().map(|p| p.done).collect::<Vec<_>>(), (1..=20).collect::<Vec<_>>());
    assert!(seen.iter().all(|p| p.total == 20));
}

#[test]
fn cancellation_is_honoured_before_and_during_a_run() {
    let (scene, tx) = room(false);
    let token = CancelToken::new();
    token.cancel();
    let control = Control { cancel: Some(&token), progress: None };
    assert_eq!(rssi_grid(&scene, &PropagationModel::default(), &tx[0], &request(0), &control), Err(PredictError::Cancelled));

    let token = CancelToken::new();
    let calls = AtomicUsize::new(0);
    let report = |_: Progress| {
        calls.fetch_add(1, Ordering::Relaxed);
        token.cancel();
    };
    let control = Control { cancel: Some(&token), progress: Some(&report) };
    let big = GridRequest { width_m: 200.0, height_m: 400.0, cell_m: 1.0, ..request(0) };
    assert_eq!(rssi_grid(&scene, &PropagationModel::default(), &tx[0], &big, &control), Err(PredictError::Cancelled));
    assert!(calls.load(Ordering::Relaxed) < 400, "work continued after cancel: {} rows", calls.load(Ordering::Relaxed));
}

#[test]
fn bad_requests_are_rejected() {
    let (scene, tx) = room(false);
    let model = PropagationModel::default();
    let run = |req: GridRequest| rssi_grid(&scene, &model, &tx[0], &req, &Control::default());
    assert!(run(GridRequest { cell_m: 0.0, ..request(0) }).is_err());
    assert!(run(GridRequest { width_m: f64::NAN, ..request(0) }).is_err());
    assert!(run(GridRequest { pixels_per_meter: -1.0, ..request(0) }).is_err());
    assert!(run(request(3)).is_err());
    assert!(run(GridRequest { width_m: 1e6, height_m: 1e6, ..request(0) }).is_err());
}

fn set_of(radios: &[(Band, f64, f64, f32)]) -> PredictionSet {
    // One-cell grids with chosen RSSI.
    let rg = |(band, center_mhz, width_mhz, v): &(Band, f64, f64, f32)| RadioGrid {
        ap_id: openspot_model::Uuid::new_v4(),
        radio_index: 0,
        band: *band,
        center_mhz: *center_mhz,
        width_mhz: *width_mhz,
        grid: Grid { cols: 1, rows: 1, cell_px: 1.0, values: vec![Some(*v)] },
    };
    PredictionSet { radios: radios.iter().map(rg).collect(), cols: 1, rows: 1, cell_px: 1.0 }
}

fn one(g: &Grid) -> f32 {
    g.get(0, 0).unwrap()
}

#[test]
fn strongest_count_snr_and_serving() {
    let s = set_of(&[(Band::Ghz5, 5180.0, 20.0, -60.0), (Band::Ghz5, 5200.0, 20.0, -50.0), (Band::Ghz2_4, 2437.0, 20.0, -40.0)]);
    assert_eq!(one(&s.strongest(None)), -40.0);
    assert_eq!(one(&s.strongest(Some(Band::Ghz5))), -50.0);
    assert_eq!(one(&s.ap_count(-55.0, None)), 2.0);
    assert_eq!(one(&s.ap_count(-55.0, Some(Band::Ghz5))), 1.0);
    assert_eq!(one(&s.snr(-95.0, Some(Band::Ghz5))), 45.0);
    assert_eq!(one(&s.serving_radio(Some(Band::Ghz5))), 1.0);
    assert!(s.strongest(Some(Band::Ghz6)).get(0, 0).is_none());
}

#[test]
fn sir_matches_the_co_channel_model() {
    // Same channel: SIR is the dB difference.
    let s = set_of(&[(Band::Ghz5, 5180.0, 20.0, -50.0), (Band::Ghz5, 5180.0, 20.0, -60.0)]);
    assert!((one(&s.sir(-90.0, None)) - 10.0).abs() < 1e-4);
    // Adjacent non-overlapping channels do not interfere.
    let s = set_of(&[(Band::Ghz5, 5180.0, 20.0, -50.0), (Band::Ghz5, 5200.0, 20.0, -52.0)]);
    assert_eq!(one(&s.sir(-90.0, None)), MAX_SIR_DB as f32);
    // Different band does not interfere.
    let s = set_of(&[(Band::Ghz5, 5180.0, 20.0, -50.0), (Band::Ghz2_4, 2437.0, 20.0, -45.0)]);
    assert_eq!(one(&s.sir(-90.0, Some(Band::Ghz5))), MAX_SIR_DB as f32);
    // 20 MHz interferer inside an 80 MHz serving channel counts for a quarter of its power.
    let s = set_of(&[(Band::Ghz5, 5210.0, 80.0, -50.0), (Band::Ghz5, 5180.0, 20.0, -60.0)]);
    let expected = 10.0 + 10.0 * 4f64.log10();
    assert!((f64::from(one(&s.sir(-90.0, None))) - expected).abs() < 1e-4);
    // Two equal interferers add 3 dB of interference power.
    let s = set_of(&[(Band::Ghz5, 5180.0, 20.0, -50.0), (Band::Ghz5, 5180.0, 20.0, -60.0), (Band::Ghz5, 5180.0, 20.0, -60.0)]);
    assert!((f64::from(one(&s.sir(-90.0, None))) - (10.0 - 10.0 * 2f64.log10())).abs() < 1e-4);
    // Below the heard threshold: ignored; nothing heard: empty.
    let s = set_of(&[(Band::Ghz5, 5180.0, 20.0, -50.0), (Band::Ghz5, 5180.0, 20.0, -95.0)]);
    assert_eq!(one(&s.sir(-90.0, None)), MAX_SIR_DB as f32);
    assert!(s.sir(-40.0, None).get(0, 0).is_none());
}

#[test]
fn computed_set_has_one_grid_per_radio_on_the_shared_lattice() {
    let (scene, mut tx) = room(false);
    let mut second = tx[0].clone();
    second.radio = Radio::new(Band::Ghz2_4, 6, 20, 15.0);
    tx.push(second);
    let set = PredictionSet::compute(&scene, &PropagationModel::default(), &tx, &request(0), &Control::default()).unwrap();
    assert_eq!((set.radios.len(), set.cols, set.rows, set.cell_px), (2, 20, 20, 10.0));
    assert_eq!(set.strongest(Some(Band::Ghz5)), set.radios[0].grid);
    assert_eq!(set.strongest(Some(Band::Ghz2_4)), set.radios[1].grid);
    // The 2.4 GHz radio at 15 dBm is weaker than 5 GHz at 20 dBm only if its loss/power say so; check union is max.
    let all = set.strongest(None);
    for i in 0..400 {
        let m = set.radios.iter().map(|r| r.grid.values[i].unwrap()).fold(f32::MIN, f32::max);
        assert_eq!(all.values[i], Some(m));
    }
}

#[test]
fn single_bounce_mode_never_lowers_a_cell_and_helps_near_walls() {
    let (_, tx) = room(false);
    let mut l0 = PlanningData::default();
    l0.materials.push(Material::custom("mirror", "mirror", MaterialCategory::Wall, 10.0, 0.7));
    l0.walls.push(Wall::new(Point::new(0.0, 4.0), Point::new(20.0, 4.0), WallKind::Wall, "mirror"));
    let scene = Scene::build(&[&l0], &MaterialCatalog::builtin().unwrap()).unwrap();
    let model = PropagationModel::default();
    let direct = rssi_grid(&scene, &model, &tx[0], &request(0), &Control::default()).unwrap();
    let bounce_req = GridRequest { mode: Mode::SingleBounce, ..request(0) };
    let bounce = rssi_grid(&scene, &model, &tx[0], &bounce_req, &Control::default()).unwrap();
    let mut better = 0;
    for (d, b) in direct.values.iter().zip(&bounce.values) {
        assert!(b.unwrap() >= d.unwrap() - 1e-4);
        better += usize::from(b.unwrap() > d.unwrap() + 0.01);
    }
    assert!(better > 20, "only {better} cells changed");
}
