//! End to end through the planning state API: draw a two-room scene, predict, check numbers.

use std::time::{Duration, Instant};

use openspot_model::{Band, MapImage, Point, Project, Scale};
use openspot_predict::fspl_db;

use super::materials::MaterialDraft;
use super::results::{ViewMode, ViewSettings};
use super::*;

const PX_PER_M: f64 = 50.0;

/// A white 20 x 10 m plan at 50 px/m.
fn blank_project() -> Project {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1000, 500, image::Rgba([255, 255, 255, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let mut p = Project::new("e2e");
    p.zones[0].map = Some(MapImage { original_name: "plan.png".into(), data: png.into_inner(), width: 1000, height: 500 });
    p.zones[0].scale = Scale::new(PX_PER_M);
    p
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// Draws the dividing wall at x = 10 m and one AP at (5, 5) through the same tool events the
/// canvas sends.
fn draw_scene(p: &mut Planning, project: &mut Project) -> Uuid {
    p.settings.snap.angle = false;
    p.settings.wall_material = "concrete-wall".into();
    p.set_tool(Tool::Wall);
    p.pointer(project, 0, Pointer::Press, pt(10.0, 0.0), 0.1);
    p.pointer(project, 0, Pointer::Drag, pt(10.0, 6.0), 0.1);
    p.pointer(project, 0, Pointer::Release, pt(10.0, 10.0), 0.1);
    p.set_tool(Tool::Ap);
    p.pointer(project, 0, Pointer::Press, pt(5.0, 5.0), 0.1);
    let id = project.zones[0].planning.access_points[0].id;
    // Mount it at client height so the 3-D distance is the plan distance.
    let mut ap = project.zones[0].planning.access_points[0].clone();
    ap.height_m = 1.0;
    p.commit_ap(project, 0, id, ap);
    id
}

fn predict(p: &mut Planning, project: &Project) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        p.advance_prediction(project, 0, false);
        if p.shown_is_current(project, 0) {
            return;
        }
        assert!(Instant::now() < deadline, "prediction never finished");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Strongest 5 GHz signal at the center of the 0.1 m cell containing plan point `(x, y)`.
fn rssi5(p: &Planning, x: f64, y: f64) -> f64 {
    let pred = p.shown().unwrap().prediction.as_ref().unwrap();
    pred.set.radios.iter().filter(|r| r.band == Band::Ghz5).map(|r| f64::from(r.grid.get((x / 0.1) as usize, (y / 0.1) as usize).unwrap())).fold(f64::NEG_INFINITY, f64::max)
}

/// Cell-center distance to the AP at (5, 5) at equal heights.
fn distance(x: f64, y: f64) -> f64 {
    let (cx, cy) = (((x / 0.1).floor() + 0.5) * 0.1, ((y / 0.1).floor() + 0.5) * 0.1);
    (cx - 5.0).hypot(cy - 5.0)
}

fn analytic(x: f64, y: f64, wall_db: f64) -> f64 {
    // Generic Wi-Fi 6 indoor AP, 5 GHz: 20 dBm + 4 dBi, channel 42 (5210 MHz); default model n = 2.5.
    24.0 - (fspl_db(1.0, 5210.0) + 25.0 * distance(x, y).max(1.0).log10() + wall_db)
}

fn concrete5() -> f64 {
    MaterialCatalog::builtin().unwrap().get("concrete-wall").unwrap().absorption_db.ghz5
}

#[test]
fn the_ap_side_room_is_stronger_and_the_wall_costs_exactly_its_configured_loss() {
    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    draw_scene(&mut p, &mut project);
    assert_eq!(project.zones[0].planning.walls.len(), 1);
    assert_eq!(project.zones[0].planning.walls[0].a, pt(10.0, 0.0), "grid snapping kept the exact corner");
    assert_eq!(project.zones[0].planning.walls[0].b, pt(10.0, 10.0));
    predict(&mut p, &project);

    let near = rssi5(&p, 7.95, 5.05);
    let far = rssi5(&p, 12.05, 5.05);
    assert!(near > far + 20.0, "{near} vs {far}");

    // Exact agreement with the formula: FSPL + 10 n log10(d) + wall, no wall for the near cell.
    assert!((near - analytic(7.95, 5.05, 0.0)).abs() < 1e-3, "near {near} vs {}", analytic(7.95, 5.05, 0.0));
    assert!((far - analytic(12.05, 5.05, concrete5())).abs() < 1e-3, "far {far} vs {}", analytic(12.05, 5.05, concrete5()));
    // The other side of the AP, still in its room: no wall, no wall term.
    let behind = rssi5(&p, 2.05, 5.05);
    assert!((behind - analytic(2.05, 5.05, 0.0)).abs() < 1e-3, "{behind}");
}

#[test]
fn editing_the_wall_material_recomputes_and_the_difference_is_the_materials_loss_difference() {
    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    draw_scene(&mut p, &mut project);
    predict(&mut p, &project);
    let (near0, far0) = (rssi5(&p, 7.95, 5.05), rssi5(&p, 12.05, 5.05));

    let wall = project.zones[0].planning.walls[0].id;
    p.edit_wall(&mut project, 0, wall, |w| w.material = "drywall".into());
    assert!(!p.shown_is_current(&project, 0), "the old result is stale after the edit");
    predict(&mut p, &project);
    let (near1, far1) = (rssi5(&p, 7.95, 5.05), rssi5(&p, 12.05, 5.05));
    let drywall5 = MaterialCatalog::builtin().unwrap().get("drywall").unwrap().absorption_db.ghz5;
    assert!((near1 - near0).abs() < 1e-6, "the AP's own room does not care about the wall");
    assert!((far1 - far0 - (concrete5() - drywall5)).abs() < 1e-3, "{} dB gained through the thinner wall", far1 - far0);

    // A custom material with a chosen loss shows up as exactly that loss.
    p.material_editor = Some(MaterialEditor { editing: None, draft: MaterialDraft { name: "Vault".into(), absorption_db: openspot_model::PerBand::splat(40.0), ..MaterialDraft::new_wall() }, error: None });
    assert!(p.save_material(&mut project, 0));
    let vault = project.zones[0].planning.materials[0].id.clone();
    p.edit_wall(&mut project, 0, wall, |w| w.material = vault.clone());
    predict(&mut p, &project);
    assert!((rssi5(&p, 12.05, 5.05) - analytic(12.05, 5.05, 40.0)).abs() < 1e-3);

    // Undo walks back through the same numbers.
    assert!(p.undo(&mut project) && p.undo(&mut project) && p.undo(&mut project));
    predict(&mut p, &project);
    assert!((rssi5(&p, 12.05, 5.05) - far0).abs() < 1e-6, "undo restored the original prediction");
}

#[test]
fn a_door_in_the_wall_lets_signal_through_with_the_doors_loss() {
    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    draw_scene(&mut p, &mut project);
    p.set_tool(Tool::Door);
    // Door centered at y = 5 m on the 10 m wall.
    p.pointer(&mut project, 0, Pointer::Press, pt(10.0, 5.0), 0.2);
    assert_eq!(project.zones[0].planning.walls[0].openings.len(), 1);
    predict(&mut p, &project);
    let door5 = MaterialCatalog::builtin().unwrap().get("wood-door").unwrap().absorption_db.ghz5;
    assert!((rssi5(&p, 12.05, 5.05) - analytic(12.05, 5.05, door5)).abs() < 1e-3, "path through the door pays the door's loss");
    assert!((rssi5(&p, 12.05, 1.05) - analytic(12.05, 1.05, concrete5())).abs() < 1e-3, "path through solid wall pays the wall's loss");
}

#[test]
fn a_second_floor_ap_reaches_the_first_floor_through_the_slab_and_a_stairwell_removes_it() {
    let mut project = blank_project();
    project.zones.push({
        let mut z = openspot_model::Zone::new("Upstairs");
        z.map = project.zones[0].map.clone();
        z.scale = project.zones[0].scale;
        z
    });
    let (ground, upstairs) = (project.zones[0].id, project.zones[1].id);
    let building = project.structure.add_building("HQ");
    assert!(project.structure.move_zone_to_building(ground, building) && project.structure.move_zone_to_building(upstairs, building));
    project.structure.buildings[0].floors.iter_mut().for_each(|f| f.height_m = 3.0);
    // One AP upstairs at (5, 5), 1.0 m above its floor; nothing on the ground floor.
    let mut model = ApModel { name: "t".into(), height_m: 1.0, radios: vec![] };
    model.radios.push(openspot_model::Radio { antenna_gain_dbi: 4.0, ..openspot_model::Radio::new(Band::Ghz5, 42, 80, 20.0) });
    project.zones[1].planning.access_points.push(model.instantiate("Up", pt(5.0, 5.0)));

    let mut p = Planning::default();
    p.sync_zone(project.id, ground);
    predict(&mut p, &project);
    let slab = MaterialCatalog::builtin().unwrap().get("concrete-slab").unwrap().absorption_db.ghz5;
    // Directly below the AP: 3 m of vertical distance (AP at 3 + 1 m, client at 1 m), one slab.
    let below = rssi5(&p, 5.05, 5.05);
    let d = (3.0f64.powi(2) + 0.05f64.powi(2) * 2.0).sqrt();
    let want = 24.0 - (fspl_db(1.0, 5210.0) + 25.0 * d.log10() + slab);
    assert!((below - want).abs() < 1e-3, "{below} vs {want}");

    // A stairwell around that spot removes the slab loss for this path. Cutouts live on the
    // level above the slab, i.e. the AP's floor.
    p.sync_zone(project.id, upstairs);
    p.set_tool(Tool::Cutout);
    for (x, y) in [(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)] {
        p.pointer(&mut project, 1, Pointer::Press, pt(x, y), 0.1);
    }
    p.confirm(&mut project, 1, 0.1);
    assert_eq!(project.zones[1].planning.cutouts.len(), 1);
    p.sync_zone(project.id, ground);
    predict(&mut p, &project);
    let open = rssi5(&p, 5.05, 5.05);
    assert!((open - (want + slab)).abs() < 1e-3, "through the stairwell {open} vs {}", want + slab);
}

#[test]
fn views_and_png_export_reflect_the_prediction() {
    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    assert!(p.export_png(&project, 0).unwrap_err().contains("no prediction"));
    let id = draw_scene(&mut p, &mut project);
    predict(&mut p, &project);

    p.view_settings = ViewSettings { mode: ViewMode::Radio { ap: id, radio_index: 1 }, opacity: 255, ..ViewSettings::default() };
    let png = p.export_png(&project, 0).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!((img.width(), img.height()), (1000, 500));
    assert_ne!(img.get_pixel(300, 250), &image::Rgba([255, 255, 255, 255]), "the AP's room is colored");
    // The marker sits on the AP: (5 m, 5 m) = pixel (250, 250).
    assert_eq!(img.get_pixel(250, 250), &image::Rgba([30, 110, 235, 255]));
    // The strong zone near the AP and the weak far room get different colors.
    assert_ne!(img.get_pixel(300, 250), img.get_pixel(900, 250));

    // Switching the view changes the export; the prediction itself is not recomputed.
    p.view_settings.mode = ViewMode::ApCount;
    assert_ne!(p.export_png(&project, 0).unwrap(), png);
}

#[test]
fn nothing_to_predict_without_an_ap_and_missing_scale_is_reported() {
    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    predict(&mut p, &project);
    assert!(p.shown().unwrap().prediction.is_none() && p.predict_error.is_none(), "an empty plan is not an error");
    project.zones[0].scale = None;
    let mut q = Planning::default();
    q.sync_zone(project.id, project.zones[0].id);
    q.advance_prediction(&project, 0, false);
    assert!(q.predict_error.as_deref().unwrap().contains("scale"));
}

#[test]
fn calibrating_to_a_survey_and_applying_makes_predictions_match_it() {
    use openspot_model::ScanPoint;
    use openspot_predict::PropagationModel;
    use openspot_wlan::{AccessPoint, MacAddress};

    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    draw_scene(&mut p, &mut project);

    // "Measure" a building whose real propagation is steeper than the default model.
    let truth = PropagationModel { exponent: 3.4, offset_db: 6.0 };
    p.predict_settings.model = truth;
    predict(&mut p, &project);
    let spots = [(6.05, 5.05), (7.55, 8.05), (9.45, 2.05), (3.05, 1.05), (12.05, 5.05), (14.05, 8.05), (17.05, 3.05), (19.05, 9.05)];
    let mut snap = openspot_model::Snapshot::new("walk");
    for (x, y) in spots {
        let rssi = rssi5(&p, x, y).round() as i32;
        let ap = AccessPoint::from_beacon("lab".into(), MacAddress([0, 0, 0, 0, 0, 1]), rssi, 5_210_000, true, 100, vec![]);
        snap.points.push(ScanPoint::new(x * PX_PER_M, y * PX_PER_M, 0, vec![ap]));
    }
    project.zones[0].snapshots = vec![snap];

    // Back to the default model, then calibrate.
    p.reset_calibration();
    predict(&mut p, &project);
    let default_far = rssi5(&p, 17.05, 3.05);
    p.run_calibration(&project, 0);
    let report = p.calibration.outcome.clone().unwrap().unwrap();
    assert!(report.after_rmse_db < report.before_rmse_db, "{} -> {}", report.before_rmse_db, report.after_rmse_db);
    assert!((report.fit.exponent - truth.exponent).abs() < 0.15, "{}", report.fit.exponent);

    p.apply_calibration();
    assert!(!p.shown_is_current(&project, 0), "a new model makes the shown prediction stale");
    predict(&mut p, &project);
    for (x, y) in spots {
        let predicted = rssi5(&p, x, y);
        let measured = f64::from(project.zones[0].snapshots[0].points.iter().find(|pt| (pt.x - x * PX_PER_M).abs() < 1e-9 && (pt.y - y * PX_PER_M).abs() < 1e-9).unwrap().access_points[0].rssi_dbm);
        assert!((predicted - measured).abs() < 2.0, "({x}, {y}): predicted {predicted:.1} vs measured {measured}");
    }
    assert!((rssi5(&p, 17.05, 3.05) - default_far).abs() > 1.0, "the calibrated map differs from the default one");

    // Reset puts the default numbers back exactly.
    p.reset_calibration();
    predict(&mut p, &project);
    assert_eq!(rssi5(&p, 17.05, 3.05), default_far);
    assert!(p.calibration.outcome.is_none());
}

#[test]
fn calibration_without_enough_points_leaves_the_model_alone() {
    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    draw_scene(&mut p, &mut project);
    project.zones[0].snapshots = vec![openspot_model::Snapshot::new("empty")];
    p.run_calibration(&project, 0);
    let err = p.calibration.outcome.clone().unwrap().unwrap_err();
    assert!(err.contains("at least 3"), "{err}");
    p.apply_calibration();
    assert_eq!(p.predict_settings.model, openspot_predict::PropagationModel::default(), "nothing to apply");
}

#[test]
fn accepting_a_suggestion_adds_a_real_ap_that_covers_the_uncovered_room() {
    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    draw_scene(&mut p, &mut project);
    let before = project.zones[0].planning.access_points.len();

    p.suggest.params = suggest::SuggestParams { count: 1, threshold_dbm: -67.0, ..Default::default() };
    p.start_suggestions(&project, 0);
    let deadline = Instant::now() + Duration::from_secs(120);
    while p.suggestions_progress().is_some() {
        p.poll_suggestions();
        assert!(Instant::now() < deadline, "suggestions never finished");
        std::thread::sleep(Duration::from_millis(2));
    }
    let zone_id = project.zones[0].id;
    let picks = p.suggestions_for(zone_id).to_vec();
    assert_eq!(picks.len(), 1, "{:?}", p.suggest.results);
    assert!(picks[0].position.x > 10.0, "the first AP covers the room without one");
    assert!(p.suggestions_are_current());
    assert!(p.suggestions_for(Uuid::new_v4()).is_empty(), "proposals belong to the zone they were computed for");

    assert!(p.accept_suggestion(&mut project, 0, 0));
    assert_eq!(project.zones[0].planning.access_points.len(), before + 1);
    let added = project.zones[0].planning.access_points[before].clone();
    assert_eq!((added.position, added.name.as_str(), &added.radios), (picks[0].position, "AP 2", &p.settings.ap_model.radios));
    assert!(p.suggestions_for(zone_id).is_empty(), "an accepted proposal leaves the list");
    assert!(!p.accept_suggestion(&mut project, 0, 0), "nothing left to accept");
    assert_eq!(p.editor.selection, Some(Selection::Ap(added.id)));

    // The new AP really covers where it was put, at the threshold it was asked for.
    predict(&mut p, &project);
    let (x, y) = (picks[0].position.x, picks[0].position.y);
    assert!(rssi5(&p, x, y) >= -67.0, "{} dBm at the suggested spot", rssi5(&p, x, y));
    assert!(p.undo(&mut project));
    assert_eq!(project.zones[0].planning.access_points.len(), before, "accepting is one undo step");
}

#[test]
fn accept_all_takes_every_proposal_in_order_and_failures_are_reported() {
    let mut project = blank_project();
    let mut p = Planning::default();
    p.sync_zone(project.id, project.zones[0].id);
    p.suggest.params = suggest::SuggestParams { count: 2, threshold_dbm: -45.0, ..Default::default() };
    p.start_suggestions(&project, 0);
    let deadline = Instant::now() + Duration::from_secs(120);
    while p.suggestions_progress().is_some() {
        p.poll_suggestions();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let picks = p.suggestions_for(project.zones[0].id).to_vec();
    assert_eq!(picks.len(), 2);
    p.accept_all_suggestions(&mut project, 0);
    let aps = &project.zones[0].planning.access_points;
    assert_eq!(aps.iter().map(|a| a.position).collect::<Vec<_>>(), vec![picks[0].position, picks[1].position]);
    assert_eq!(aps.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["AP 1", "AP 2"]);

    project.zones[0].scale = None;
    p.start_suggestions(&project, 0);
    assert!(p.suggest.results.clone().unwrap().unwrap_err().contains("scale"));
    p.clear_suggestions();
    assert!(p.suggest.results.is_none());
}
