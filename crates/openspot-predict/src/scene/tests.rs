use openspot_model::{Cutout, Material, MaterialCategory, Opening, OpeningKind, Slab, Wall, WallKind};

use super::*;

fn catalog() -> MaterialCatalog {
    MaterialCatalog::builtin().unwrap()
}

fn mat(id: &str, db: f64) -> Material {
    Material::custom(id, id, MaterialCategory::Wall, db, 0.5)
}

fn level(elevation: f64) -> PlanningData {
    let mut d = PlanningData::default();
    d.level.elevation_m = elevation;
    d.level.slab = Slab { thickness_m: 0.2, material: "slab".into() };
    d.materials.push(Material::custom("slab", "slab", MaterialCategory::Slab, 20.0, 0.5));
    d.materials.push(mat("w3", 3.0));
    d.materials.push(mat("w5", 5.0));
    d.materials.push(mat("w7", 7.0));
    d
}

fn vwall(d: &mut PlanningData, x: f64, material: &str) {
    d.walls.push(Wall::new(Point::new(x, -50.0), Point::new(x, 50.0), WallKind::Wall, material));
}

fn pos(level: usize, x: f64, y: f64, h: f64) -> Position {
    Position { level, x, y, height_m: h }
}

fn loss(scene: &Scene, a: Position, b: Position, band: Band) -> PathLoss {
    scene.path_loss(&PropagationModel::default(), a, b, 2437.0, band).unwrap()
}

#[test]
fn each_wall_loss_is_applied_once_per_crossing_and_summed() {
    let mut d = level(0.0);
    vwall(&mut d, 2.0, "w3");
    vwall(&mut d, 4.0, "w5");
    vwall(&mut d, 6.0, "w7");
    vwall(&mut d, 20.0, "w7"); // beyond the receiver
    let scene = Scene::build(&[&d], &catalog()).unwrap();
    let l = loss(&scene, pos(0, 0.0, 0.0, 1.0), pos(0, 8.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!((l.wall_crossings, l.wall_db), (3, 15.0));
    // Only the first two walls lie between x=0 and x=5.
    let l = loss(&scene, pos(0, 0.0, 0.0, 1.0), pos(0, 5.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!((l.wall_crossings, l.wall_db), (2, 8.0));
    // Reciprocal.
    let back = loss(&scene, pos(0, 5.0, 0.0, 1.0), pos(0, 0.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!(back.wall_db, 8.0);
}

#[test]
fn loss_is_taken_for_the_radio_band() {
    let mut d = level(0.0);
    let mut m = mat("banded", 0.0);
    m.absorption_db = PerBand { ghz2_4: 4.0, ghz5: 9.0, ghz6: 12.0 };
    d.materials.push(m);
    vwall(&mut d, 3.0, "banded");
    let scene = Scene::build(&[&d], &catalog()).unwrap();
    let (a, b) = (pos(0, 0.0, 0.0, 1.0), pos(0, 6.0, 0.0, 1.0));
    assert_eq!(loss(&scene, a, b, Band::Ghz2_4).wall_db, 4.0);
    assert_eq!(loss(&scene, a, b, Band::Ghz5).wall_db, 9.0);
    assert_eq!(loss(&scene, a, b, Band::Ghz6).wall_db, 12.0);
}

#[test]
fn paths_that_miss_walls_pay_nothing_and_wall_ends_are_respected() {
    let mut d = level(0.0);
    d.walls.push(Wall::new(Point::new(3.0, 1.0), Point::new(3.0, 5.0), WallKind::Wall, "w5"));
    let scene = Scene::build(&[&d], &catalog()).unwrap();
    assert_eq!(loss(&scene, pos(0, 0.0, 0.0, 1.0), pos(0, 6.0, 0.0, 1.0), Band::Ghz5).wall_crossings, 0);
    assert_eq!(loss(&scene, pos(0, 0.0, 3.0, 1.0), pos(0, 6.0, 3.0, 1.0), Band::Ghz5).wall_crossings, 1);
    // Parallel to the wall.
    assert_eq!(loss(&scene, pos(0, 2.0, 0.0, 1.0), pos(0, 2.0, 8.0, 1.0), Band::Ghz5).wall_crossings, 0);
}

#[test]
fn ap_mounted_on_a_wall_is_not_penalised_by_it() {
    let mut d = level(0.0);
    vwall(&mut d, 0.0, "w5");
    let scene = Scene::build(&[&d], &catalog()).unwrap();
    assert_eq!(loss(&scene, pos(0, 0.0, 0.0, 2.0), pos(0, 5.0, 1.0, 1.0), Band::Ghz5).wall_crossings, 0);
}

#[test]
fn opening_replaces_the_wall_material_only_within_its_span() {
    let mut d = level(0.0);
    d.materials.push(Material::custom("door", "door", MaterialCategory::Door, 1.0, 0.1));
    let mut w = Wall::new(Point::new(5.0, 0.0), Point::new(5.0, 10.0), WallKind::Wall, "w7");
    w.openings.push(Opening { id: openspot_model::Uuid::new_v4(), kind: OpeningKind::Door, start: 0.4, end: 0.6, material: "door".into() });
    d.walls.push(w);
    let scene = Scene::build(&[&d], &catalog()).unwrap();
    let through = |y: f64| loss(&scene, pos(0, 0.0, y, 1.0), pos(0, 10.0, y, 1.0), Band::Ghz5).wall_db;
    assert_eq!(through(5.0), 1.0); // u = 0.5, inside the door
    assert_eq!(through(4.0), 1.0); // u = 0.4, door edge inclusive
    assert_eq!(through(2.0), 7.0); // u = 0.2
    assert_eq!(through(8.0), 7.0);
}

#[test]
fn slab_loss_counts_each_slab_and_cutouts_bypass_it() {
    let (l0, l1, mut l2) = (level(0.0), level(3.0), level(6.0));
    let scene = Scene::build(&[&l0, &l1, &l2], &catalog()).unwrap();
    let a = pos(0, 0.0, 0.0, 1.0);
    let up1 = loss(&scene, a, pos(1, 4.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!((up1.slab_crossings, up1.floor_db), (1, 20.0));
    let up2 = loss(&scene, a, pos(2, 4.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!((up2.slab_crossings, up2.floor_db), (2, 40.0));
    // Downward is the same.
    let down = loss(&scene, pos(2, 4.0, 0.0, 1.0), a, Band::Ghz5);
    assert_eq!((down.slab_crossings, down.floor_db), (2, 40.0));
    // Same level: no slab.
    assert_eq!(loss(&scene, a, pos(0, 4.0, 0.0, 1.0), Band::Ghz5).floor_db, 0.0);

    // A stairwell through the slab of level 2 where the 1 -> 2 crossing happens.
    // Path from (0,0,z=1) to (4,0,z=7): slab 3 at t=1/3 (x=1.33), slab 6 at t=5/6 (x=3.33).
    l2.cutouts.push(Cutout { id: openspot_model::Uuid::new_v4(), polygon: vec![Point::new(3.0, -1.0), Point::new(4.0, -1.0), Point::new(4.0, 1.0), Point::new(3.0, 1.0)] });
    let scene = Scene::build(&[&l0, &l1, &l2], &catalog()).unwrap();
    let r = loss(&scene, a, pos(2, 4.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!((r.slab_crossings, r.floor_db), (1, 20.0));
    // Cutout polygon elsewhere: no bypass.
    let mut l2b = level(6.0);
    l2b.cutouts.push(Cutout { id: openspot_model::Uuid::new_v4(), polygon: vec![Point::new(8.0, 8.0), Point::new(9.0, 8.0), Point::new(9.0, 9.0)] });
    let scene = Scene::build(&[&l0, &l1, &l2b], &catalog()).unwrap();
    assert_eq!(loss(&scene, a, pos(2, 4.0, 0.0, 1.0), Band::Ghz5).floor_db, 40.0);
}

#[test]
fn walls_apply_only_to_the_part_of_the_path_inside_their_level() {
    // TX (0,0) h=1 on level 0 (z=1); RX (10,0) h=1 on level 1 (z=4); slab at z=3 -> x = 6.67.
    let (mut l0, mut l1) = (level(0.0), level(3.0));
    vwall(&mut l0, 3.0, "w5"); // on the level-0 part
    vwall(&mut l0, 8.0, "w7"); // level-0 wall past the slab crossing: path is upstairs there
    vwall(&mut l1, 8.0, "w3"); // level-1 part
    vwall(&mut l1, 3.0, "w7"); // level-1 wall below the path's early part
    let scene = Scene::build(&[&l0, &l1], &catalog()).unwrap();
    let r = loss(&scene, pos(0, 0.0, 0.0, 1.0), pos(1, 10.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!(r.wall_crossings, 2);
    assert_eq!(r.wall_db, 5.0 + 3.0);
    assert_eq!(r.floor_db, 20.0);
    let back = loss(&scene, pos(1, 10.0, 0.0, 1.0), pos(0, 0.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!((back.wall_db, back.floor_db), (8.0, 20.0));
}

#[test]
fn level_alignment_offset_shifts_walls_cutouts_and_positions() {
    let (l0, mut l1) = (level(0.0), level(3.0));
    l1.level.offset = Point::new(5.0, 0.0);
    vwall(&mut l1, 3.0, "w5"); // world x = 8
    let scene = Scene::build(&[&l0, &l1], &catalog()).unwrap();
    // Same-level path on level 1 in local coords: wall at local x=3.
    let a = loss(&scene, pos(1, 0.0, 0.0, 1.0), pos(1, 6.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!(a.wall_crossings, 1);
    // Level-0 TX at x=6 (world 6) up to level-1 RX at local x=0 (world 5): no wall between world 5..6.
    let b = loss(&scene, pos(0, 6.0, 0.0, 1.0), pos(1, 0.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!(b.wall_crossings, 0);
    // Distance reflects the world offset: local x equal, world 5 m apart plus 3 m vertical.
    let c = loss(&scene, pos(0, 0.0, 0.0, 1.0), pos(1, 0.0, 0.0, 1.0), Band::Ghz5);
    assert!((c.distance_m - 34f64.sqrt()).abs() < 1e-12);
}

#[test]
fn distance_is_three_dimensional() {
    let d = level(0.0);
    let scene = Scene::build(&[&d], &catalog()).unwrap();
    let r = loss(&scene, pos(0, 0.0, 0.0, 1.0), pos(0, 3.0, 4.0, 13.0), Band::Ghz5);
    assert!((r.distance_m - 13.0).abs() < 1e-12);
    assert!((r.total_db() - PropagationModel::default().distance_loss_db(13.0, 2437.0)).abs() < 1e-12);
}

#[test]
fn bad_scenes_are_rejected() {
    let mut d = level(0.0);
    vwall(&mut d, 1.0, "no-such-material");
    assert_eq!(Scene::build(&[&d], &catalog()).unwrap_err(), PredictError::UnknownMaterial("no-such-material".into()));
    let (a, b) = (level(3.0), level(3.0));
    assert!(matches!(Scene::build(&[&a, &b], &catalog()), Err(PredictError::InvalidInput(_))));
    let ok = level(0.0);
    let scene = Scene::build(&[&ok], &catalog()).unwrap();
    assert!(scene.path_loss(&PropagationModel::default(), pos(4, 0.0, 0.0, 1.0), pos(0, 0.0, 0.0, 1.0), 2437.0, Band::Ghz5).is_err());
}

#[test]
fn preset_catalog_materials_resolve_for_walls_and_default_slab() {
    let mut d = level(0.0);
    d.materials.clear();
    d.level.slab.material = "concrete-slab".into();
    vwall(&mut d, 3.0, "concrete-wall");
    let scene = Scene::build(&[&d], &catalog()).unwrap();
    let r = loss(&scene, pos(0, 0.0, 0.0, 1.0), pos(0, 6.0, 0.0, 1.0), Band::Ghz5);
    assert_eq!(r.wall_db, catalog().get("concrete-wall").unwrap().absorption_db.ghz5);
}

#[test]
fn transmitters_list_enabled_radios_and_reject_bad_patterns() {
    let mut d = level(0.0);
    let mut off = Radio::new(Band::Ghz5, 36, 80, 17.0);
    off.enabled = false;
    d.access_points.push(VirtualAp::new("a", Point::new(1.0, 2.0), vec![Radio::new(Band::Ghz2_4, 1, 20, 15.0), off]));
    let tx = Scene::transmitters(&[&d]).unwrap();
    assert_eq!(tx.len(), 1);
    assert_eq!((tx[0].radio_index, tx[0].position), (0, pos(0, 1.0, 2.0, 2.5)));

    let mut bad = Radio::new(Band::Ghz5, 36, 80, 17.0);
    bad.pattern = AntennaPattern::Directional { azimuth_deg: 0.0, h_beamwidth_deg: 0.0, v_beamwidth_deg: 30.0, tilt_deg: 0.0, front_to_back_db: 25.0 };
    d.access_points.push(VirtualAp::new("b", Point::new(0.0, 0.0), vec![bad]));
    assert!(Scene::transmitters(&[&d]).is_err());
}
