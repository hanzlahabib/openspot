use openspot_model::{MapImage, Point, Project, Zone};

use super::materials::MaterialDraft;
use super::*;

fn project() -> Project {
    let mut p = Project::new("t");
    p.zones[0].scale = Scale::new(50.0);
    p
}

fn planning(p: &Project) -> Planning {
    let mut planning = Planning::default();
    planning.sync_zone(p.id, p.zones[0].id);
    planning
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

#[test]
fn drawing_through_the_state_api_marks_changes_and_undoes() {
    let mut project = project();
    let mut p = planning(&project);
    p.set_tool(Tool::Wall);
    let g0 = p.generation();
    p.pointer(&mut project, 0, Pointer::Press, pt(1.0, 1.0), 0.1);
    p.pointer(&mut project, 0, Pointer::Drag, pt(5.0, 1.0), 0.1);
    p.pointer(&mut project, 0, Pointer::Release, pt(5.0, 1.0), 0.1);
    assert_eq!(project.zones[0].planning.walls.len(), 1);
    assert!(p.generation() > g0 && p.take_changed() && !p.take_changed(), "changed is reported once");
    assert!(p.can_undo());
    assert!(p.undo(&mut project));
    assert!(project.zones[0].planning.walls.is_empty());
    assert!(p.editor.selection.is_none(), "the selected wall is gone with the undo");
    assert!(p.redo(&mut project) && project.zones[0].planning.walls.len() == 1);
}

#[test]
fn switching_zone_drops_selection_and_half_drawn_walls_but_keeps_history() {
    let mut project = project();
    project.zones.push(Zone::new("two"));
    let mut p = planning(&project);
    p.set_tool(Tool::Wall);
    p.pointer(&mut project, 0, Pointer::Press, pt(0.0, 0.0), 0.1);
    p.pointer(&mut project, 0, Pointer::Release, pt(0.0, 0.0), 0.1);
    assert!(p.editor.is_drafting());
    p.sync_zone(project.id, project.zones[1].id);
    assert!(!p.editor.is_drafting() && p.needs_fit);
}

#[test]
fn edits_to_a_wall_property_are_single_undo_steps() {
    let mut project = project();
    let mut p = planning(&project);
    p.set_tool(Tool::Wall);
    p.pointer(&mut project, 0, Pointer::Press, pt(0.0, 0.0), 0.1);
    p.pointer(&mut project, 0, Pointer::Release, pt(4.0, 0.0), 0.1);
    let id = project.zones[0].planning.walls[0].id;
    p.edit_wall(&mut project, 0, id, |w| w.material = "concrete-wall".into());
    p.edit_wall(&mut project, 0, id, |w| w.material = "concrete-wall".into());
    assert_eq!(project.zones[0].planning.walls[0].material, "concrete-wall");
    assert!(p.undo(&mut project), "the no-op second edit left no history entry");
    assert_eq!(project.zones[0].planning.walls[0].material, "drywall");
    assert!(p.undo(&mut project));
    assert!(!p.can_undo());
}

#[test]
fn the_material_editor_creates_updates_and_deletes_with_undo() {
    let mut project = project();
    let mut p = planning(&project);
    p.open_material_editor();
    let e = p.material_editor.as_mut().unwrap();
    e.draft.name = "Foam".into();
    e.draft.absorption_db = openspot_model::PerBand::splat(7.0);
    assert!(p.save_material(&mut project, 0));
    let id = p.material_editor.as_ref().unwrap().editing.clone().expect("the editor now edits the new material");
    assert_eq!(project.zones[0].planning.materials[0].absorption_db.ghz5, 7.0);

    p.material_editor.as_mut().unwrap().draft.absorption_db.ghz5 = 9.0;
    assert!(p.save_material(&mut project, 0));
    assert_eq!(project.zones[0].planning.materials[0].absorption_db.ghz5, 9.0);

    p.material_editor.as_mut().unwrap().draft.name = "  ".into();
    assert!(!p.save_material(&mut project, 0));
    assert!(p.material_editor.as_ref().unwrap().error.is_some());

    p.delete_material(&mut project, 0, &id);
    assert!(project.zones[0].planning.materials.is_empty() && p.material_editor.is_none());
    assert!(p.undo(&mut project), "deletion undoes");
    assert_eq!(project.zones[0].planning.materials[0].absorption_db.ghz5, 9.0);
}

#[test]
fn a_material_in_use_cannot_be_deleted() {
    let mut project = project();
    let mut p = planning(&project);
    p.material_editor = Some(MaterialEditor { editing: None, draft: MaterialDraft { name: "Foam".into(), ..MaterialDraft::new_wall() }, error: None });
    assert!(p.save_material(&mut project, 0));
    let id = project.zones[0].planning.materials[0].id.clone();
    p.set_tool(Tool::Wall);
    p.settings.wall_material = id.clone();
    p.pointer(&mut project, 0, Pointer::Press, pt(0.0, 0.0), 0.1);
    p.pointer(&mut project, 0, Pointer::Release, pt(4.0, 0.0), 0.1);
    p.delete_material(&mut project, 0, &id);
    assert_eq!(project.zones[0].planning.materials.len(), 1);
    assert!(p.notice.as_deref().unwrap().contains("used by"));
}

#[test]
fn custom_materials_can_be_copied_to_the_other_floors_of_a_building() {
    let mut project = project();
    project.zones.push(Zone::new("upstairs"));
    let (a, b) = (project.zones[0].id, project.zones[1].id);
    let building = project.structure.add_building("HQ");
    assert!(project.structure.move_zone_to_building(a, building) && project.structure.move_zone_to_building(b, building));
    let mut p = planning(&project);
    p.material_editor = Some(MaterialEditor { editing: None, draft: MaterialDraft { name: "Foam".into(), ..MaterialDraft::new_wall() }, error: None });
    p.save_material(&mut project, 0);
    let id = project.zones[0].planning.materials[0].id.clone();
    p.copy_material_to_building(&mut project, 0, &id);
    assert_eq!(project.zones[1].planning.materials.len(), 1);
    p.copy_material_to_building(&mut project, 0, &id);
    assert!(p.notice.as_deref().unwrap().contains("already"));
    assert!(p.undo(&mut project) && project.zones[1].planning.materials.is_empty());

    // Outside a building there is nothing to copy to.
    let mut lone = Project::new("lone");
    let mut q = planning(&lone);
    q.material_editor = Some(MaterialEditor { editing: None, draft: MaterialDraft { name: "Foam".into(), ..MaterialDraft::new_wall() }, error: None });
    q.save_material(&mut lone, 0);
    let id = lone.zones[0].planning.materials[0].id.clone();
    q.copy_material_to_building(&mut lone, 0, &id);
    assert!(q.notice.as_deref().unwrap().contains("not part of a building"));
}

#[test]
fn scale_from_plan_width_uses_the_image_width() {
    let mut z = Zone::new("z");
    assert!(Planning::set_scale_from_width(&mut z, 20.0).is_err(), "no plan yet");
    z.map = Some(MapImage { original_name: "m".into(), data: vec![], width: 1000, height: 500 });
    let s = Planning::set_scale_from_width(&mut z, 20.0).unwrap();
    assert_eq!(s.pixels_per_meter(), 50.0);
    assert_eq!(z.scale, Some(s));
    assert!(Planning::set_scale_from_width(&mut z, 0.0).is_err() && Planning::set_scale_from_width(&mut z, f64::NAN).is_err());
    assert_eq!(z.scale, Some(s), "a refused width leaves the scale alone");
}

#[test]
fn prefs_round_trip_snap_thickness_and_user_models() {
    let mut p = Planning::default();
    p.settings.snap = geometry::SnapSettings { grid: false, grid_m: 0.5, angle: false, endpoints: true };
    p.settings.thickness_m = 0.2;
    let mut model = p.models.builtin()[0].clone();
    model.name = "Mine".into();
    p.models.save_user(model.clone()).unwrap();
    let prefs = p.prefs();
    let mut q = Planning::default();
    q.apply_prefs(&prefs);
    assert_eq!(q.settings.snap, p.settings.snap);
    assert_eq!((q.settings.thickness_m, q.models.user.clone()), (0.2, vec![model]));
    // Display and prediction settings travel too.
    p.view_settings.gradient = openspot_heatmap::GradientPreset::BlueRed;
    p.view_settings.smoothing = openspot_heatmap::Smoothing::Strong;
    p.view_settings.opacity = 99;
    p.auto_predict = false;
    p.predict_settings.detail = 2;
    p.predict_settings.mode = Mode::SingleBounce;
    let mut q = Planning::default();
    q.apply_prefs(&p.prefs());
    assert_eq!((q.view_settings.gradient, q.view_settings.smoothing, q.view_settings.opacity), (p.view_settings.gradient, p.view_settings.smoothing, 99));
    assert_eq!((q.auto_predict, q.predict_settings.detail, q.predict_settings.mode), (false, 2, Mode::SingleBounce));
    let fresh = Planning::default();
    assert_eq!((fresh.view_settings.smoothing, fresh.auto_predict, fresh.predict_settings.mode), (Smoothing::Off, true, Mode::Direct), "defaults");
    // Garbage from a hand-edited file is ignored rather than trusted.
    let mut bad = prefs;
    bad.snap_grid_index = 99;
    bad.thickness_m = f64::NAN;
    bad.gradient = 77;
    bad.smoothing = 77;
    bad.detail = 77;
    bad.opacity = 0;
    let mut r = Planning::default();
    r.apply_prefs(&bad);
    assert_eq!(r.settings.snap.grid_m, geometry::GRID_PRESETS_M[1]);
    assert_eq!(r.settings.thickness_m, Planning::default().settings.thickness_m);
    assert_eq!((r.view_settings.gradient, r.view_settings.smoothing), (GradientPreset::Default, Smoothing::Off));
    assert_eq!(r.predict_settings.detail, predict_input::DETAIL_CELLS.len() - 1);
    assert_eq!(r.view_settings.opacity, crate::survey::MIN_OPACITY, "an invisible overlay is raised to the minimum");
}

#[test]
fn opening_another_project_clears_history_but_the_same_project_keeps_it() {
    let mut project = project();
    let mut p = planning(&project);
    p.set_tool(Tool::Wall);
    p.pointer(&mut project, 0, Pointer::Press, pt(0.0, 0.0), 0.1);
    p.pointer(&mut project, 0, Pointer::Release, pt(4.0, 0.0), 0.1);
    p.sync_zone(project.id, project.zones[0].id);
    assert!(p.can_undo());
    let other = Project::new("other");
    p.sync_zone(other.id, other.zones[0].id);
    assert!(!p.can_undo());
}
