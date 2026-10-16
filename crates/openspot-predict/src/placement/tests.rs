use openspot_model::{Band, Material, MaterialCatalog, MaterialCategory, PlanningData, VirtualAp, Wall, WallKind};

use super::*;
use crate::grid::{CancelToken, Mode};

/// 20 x 10 m floor split in two rooms by a 40 dB wall at x = 10.
fn two_rooms() -> (Scene, PlanningData) {
    let mut d = PlanningData::default();
    d.materials.push(Material::custom("thick", "thick", MaterialCategory::Wall, 40.0, 0.0));
    d.walls.push(Wall::new(Point::new(10.0, 0.0), Point::new(10.0, 10.0), WallKind::Wall, "thick"));
    let scene = Scene::build(&[&d], &MaterialCatalog::builtin().unwrap()).unwrap();
    (scene, d)
}

fn grid() -> GridRequest {
    GridRequest { level: 0, width_m: 20.0, height_m: 10.0, cell_m: 1.0, rx_height_m: 1.0, pixels_per_meter: 10.0, mode: Mode::Direct }
}

// 10 dBm radio, 3 dBi: reaches -65 dBm about 5-6 m in the open (exponent 2.5).
fn weak_radio() -> Radio {
    Radio::new(Band::Ghz5, 36, 20, 10.0)
}

fn request<'a>(candidates: &'a [Point], radio: &'a Radio, count: usize) -> PlacementRequest<'a> {
    PlacementRequest { candidates, template: radio, height_m: 2.5, count, threshold_dbm: -65.0, grid: grid() }
}

#[test]
fn lattice_covers_the_area_with_margins() {
    let l = candidate_lattice(20.0, 10.0, 5.0, 2.5);
    // x: 2.5, 7.5, 12.5, 17.5; y: 2.5, 7.5
    assert_eq!(l.len(), 8);
    assert_eq!(l[0], Point::new(2.5, 2.5));
    assert_eq!(l[7], Point::new(17.5, 7.5));
    assert!(candidate_lattice(20.0, 10.0, 0.0, 0.0).is_empty());
    assert!(candidate_lattice(2.0, 2.0, 1.0, 5.0).is_empty());
}

#[test]
fn two_aps_are_placed_one_per_room() {
    let (scene, _) = two_rooms();
    let candidates = candidate_lattice(20.0, 10.0, 5.0, 2.5);
    let radio = weak_radio();
    let picks = suggest_placements(&scene, &PropagationModel::default(), &[], &request(&candidates, &radio, 2), &Control::default()).unwrap();
    assert_eq!(picks.len(), 2);
    assert!((picks[0].position.x < 10.0) != (picks[1].position.x < 10.0), "both APs in one room: {picks:?}");
    assert!(picks[1].covered_fraction > picks[0].covered_fraction);
    assert!(picks[0].newly_covered_cells >= picks[1].newly_covered_cells, "greedy picks the biggest gain first");
    assert_eq!(picks[1].covered_fraction, (picks[0].newly_covered_cells + picks[1].newly_covered_cells) as f64 / 200.0);
}

#[test]
fn first_pick_has_the_maximum_coverage_among_all_candidates() {
    let (scene, _) = two_rooms();
    let candidates = candidate_lattice(20.0, 10.0, 2.5, 1.25);
    let radio = weak_radio();
    let model = PropagationModel::default();
    let pick = suggest_placements(&scene, &model, &[], &request(&candidates, &radio, 1), &Control::default()).unwrap()[0];
    let best = candidates
        .iter()
        .map(|p| {
            let tx = Transmitter { ap_id: Uuid::nil(), radio_index: 0, radio: radio.clone(), position: Position { level: 0, x: p.x, y: p.y, height_m: 2.5 } };
            let g = rssi_grid(&scene, &model, &tx, &grid(), &Control::default()).unwrap();
            g.values.iter().filter(|v| v.is_some_and(|v| v >= -65.0)).count()
        })
        .max()
        .unwrap();
    assert_eq!(pick.newly_covered_cells, best);
    assert!(best > 0 && best < 200);
}

#[test]
fn existing_coverage_steers_new_aps_elsewhere_and_full_coverage_stops_early() {
    let (scene, mut d) = two_rooms();
    d.access_points.push(VirtualAp::new("left", Point::new(5.0, 5.0), vec![weak_radio()]));
    let existing = Scene::transmitters(&[&d]).unwrap();
    let candidates = candidate_lattice(20.0, 10.0, 5.0, 2.5);
    let radio = weak_radio();
    let picks = suggest_placements(&scene, &PropagationModel::default(), &existing, &request(&candidates, &radio, 5), &Control::default()).unwrap();
    assert!(!picks.is_empty() && picks.len() < 5, "should stop once nothing improves: {picks:?}");
    assert!(picks[0].position.x > 10.0, "first new AP belongs in the uncovered right room");
    let last = picks.last().unwrap();
    assert!(last.covered_fraction <= 1.0);
}

#[test]
fn other_band_aps_do_not_count_as_coverage() {
    let (scene, mut d) = two_rooms();
    d.access_points.push(VirtualAp::new("24", Point::new(5.0, 5.0), vec![Radio::new(Band::Ghz2_4, 6, 20, 20.0)]));
    let existing = Scene::transmitters(&[&d]).unwrap();
    let candidates = [Point::new(5.0, 5.0), Point::new(15.0, 5.0)];
    let radio = weak_radio();
    let picks = suggest_placements(&scene, &PropagationModel::default(), &existing, &request(&candidates, &radio, 1), &Control::default()).unwrap();
    assert_eq!(picks.len(), 1);
    assert!(picks[0].newly_covered_cells > 0);
}

#[test]
fn bad_input_and_cancellation() {
    let (scene, _) = two_rooms();
    let radio = weak_radio();
    let model = PropagationModel::default();
    let too_many = vec![Point::new(1.0, 1.0); MAX_CANDIDATES + 1];
    assert!(suggest_placements(&scene, &model, &[], &request(&too_many, &radio, 1), &Control::default()).is_err());
    let none = suggest_placements(&scene, &model, &[], &request(&[], &radio, 3), &Control::default()).unwrap();
    assert!(none.is_empty());
    let token = CancelToken::new();
    token.cancel();
    let control = Control { cancel: Some(&token), progress: None };
    let one = [Point::new(5.0, 5.0)];
    assert_eq!(suggest_placements(&scene, &model, &[], &request(&one, &radio, 1), &control), Err(PredictError::Cancelled));
}
