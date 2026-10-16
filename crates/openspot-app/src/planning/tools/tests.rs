use openspot_model::{Project, WallKind};

use super::*;
use crate::planning::ap_models::builtin_models;
use crate::planning::edit::History;

/// Drives an [`Editor`] against a real project the way the canvas does: every edit is
/// applied and recorded before the next event.
struct Harness {
    project: Project,
    editor: Editor,
    settings: ToolSettings,
    history: History,
    zone: Uuid,
    tol: f64,
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

impl Harness {
    fn new(tool: Tool) -> Self {
        let project = Project::new("t");
        let zone = project.zones[0].id;
        let mut editor = Editor::default();
        editor.set_tool(tool);
        Self { project, editor, settings: ToolSettings::new(builtin_models().remove(0)), history: History::default(), zone, tol: 0.15 }
    }

    fn data(&self) -> &PlanningData {
        &self.project.zones[0].planning
    }

    fn ev(&mut self, kind: Pointer, x: f64, y: f64) -> Outcome {
        let ctx = Context { zone: self.zone, data: &self.project.zones[0].planning, settings: &self.settings, tol_m: self.tol };
        let out = self.editor.pointer(kind, pt(x, y), &ctx);
        self.apply(&out);
        out
    }

    fn apply(&mut self, out: &Outcome) {
        for e in &out.edits {
            assert!(e.apply(&mut self.project, true));
            self.history.record(e.clone());
        }
    }

    /// Press and release at one spot; the outcome of whichever event had one.
    fn click(&mut self, x: f64, y: f64) -> Outcome {
        let press = self.ev(Pointer::Press, x, y);
        let release = self.ev(Pointer::Release, x, y);
        if press.edits.is_empty() && press.message.is_none() { release } else { press }
    }

    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) -> Outcome {
        self.ev(Pointer::Press, from.0, from.1);
        self.ev(Pointer::Drag, (from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
        self.ev(Pointer::Drag, to.0, to.1);
        self.ev(Pointer::Release, to.0, to.1)
    }

    fn confirm(&mut self) -> Outcome {
        let ctx = Context { zone: self.zone, data: &self.project.zones[0].planning, settings: &self.settings, tol_m: self.tol };
        let out = self.editor.confirm(&ctx);
        self.apply(&out);
        out
    }

    fn delete_selection(&mut self) -> Outcome {
        let ctx = Context { zone: self.zone, data: &self.project.zones[0].planning, settings: &self.settings, tol_m: self.tol };
        let out = self.editor.delete_selection(&ctx);
        self.apply(&out);
        out
    }

    fn with_wall(mut self, a: Point, b: Point) -> Self {
        self.project.zones[0].planning.walls.push(Wall::new(a, b, WallKind::Wall, "drywall"));
        self
    }
}

#[test]
fn dragging_draws_one_snapped_wall_with_the_current_material_and_thickness() {
    let mut h = Harness::new(Tool::Wall);
    h.settings.thickness_m = 0.2;
    h.settings.wall_material = "concrete-wall".into();
    h.ev(Pointer::Press, 1.03, 0.98);
    assert!(matches!(h.editor.preview(), Preview::Segment { a, b, .. } if *a == pt(1.0, 1.0) && *b == pt(1.0, 1.0)));
    h.ev(Pointer::Drag, 3.0, 1.1);
    assert!(h.data().walls.is_empty(), "nothing is created before release");
    let out = h.ev(Pointer::Release, 5.05, 1.02);
    assert_eq!(out.edits.len(), 1);
    let w = &h.data().walls[0];
    assert_eq!((w.a, w.b), (pt(1.0, 1.0), pt(5.0, 1.0)), "grid, then angle locked to horizontal with a 4.0 m length");
    assert_eq!((w.thickness_m, w.material.as_str(), w.kind), (0.2, "concrete-wall", WallKind::Wall));
    assert_eq!(h.editor.selection, Some(Selection::Wall(w.id)));
    assert!(!h.editor.is_drafting(), "a dragged wall ends the draft");
}

#[test]
fn clicking_chains_walls_until_cancelled() {
    let mut h = Harness::new(Tool::Wall);
    h.click(0.0, 0.0);
    assert!(h.editor.is_drafting() && h.data().walls.is_empty());
    h.ev(Pointer::Hover, 3.9, 0.1);
    assert!(matches!(h.editor.preview(), Preview::Segment { a, b, .. } if *a == pt(0.0, 0.0) && *b == pt(4.0, 0.0)));
    h.click(4.0, 0.0);
    h.click(4.0, 3.0);
    h.click(0.0, 3.0);
    let w = &h.data().walls;
    assert_eq!(w.len(), 3);
    assert_eq!((w[0].b, w[1].a, w[1].b, w[2].a), (pt(4.0, 0.0), pt(4.0, 0.0), pt(4.0, 3.0), pt(4.0, 3.0)), "each wall starts where the previous ended");
    h.editor.cancel();
    h.click(9.0, 9.0);
    assert_eq!(h.data().walls.len(), 3, "after Esc a click only starts a new chain");
    assert!(h.editor.is_drafting());
}

#[test]
fn an_accidental_tiny_drag_makes_no_wall() {
    let mut h = Harness::new(Tool::Wall);
    h.drag((2.0, 2.0), (2.02, 2.02));
    assert!(h.data().walls.is_empty());
    h.click(2.0, 2.0);
    assert!(h.data().walls.is_empty(), "a second click on the same spot is still too short");
}

#[test]
fn new_walls_snap_to_existing_endpoints() {
    let mut h = Harness::new(Tool::Wall).with_wall(pt(0.0, 0.0), pt(4.1, 0.0));
    h.drag((4.2, 0.1), (4.15, 3.0));
    let w = &h.data().walls[1];
    assert_eq!(w.a, pt(4.1, 0.0), "start snapped to the existing end (grid alone would give 4.25)");
}

#[test]
fn racks_use_the_shelf_kind_material_and_depth() {
    let mut h = Harness::new(Tool::Shelf);
    h.drag((1.0, 1.0), (3.0, 1.0));
    let w = &h.data().walls[0];
    assert_eq!((w.kind, w.material.as_str(), w.thickness_m), (WallKind::Shelf, "chipboard-shelf", SHELF_THICKNESS_M));
}

#[test]
fn doors_and_windows_go_into_the_wall_under_the_pointer() {
    let mut h = Harness::new(Tool::Door).with_wall(pt(0.0, 0.0), pt(6.0, 0.0));
    h.click(2.0, 0.05);
    let w = &h.data().walls[0];
    assert_eq!(w.openings.len(), 1);
    let o = &w.openings[0];
    assert_eq!(o.material, "wood-door");
    assert!(((o.end - o.start) * 6.0 - 0.9).abs() < 1e-9 && ((o.start + o.end) / 2.0 - 2.0 / 6.0).abs() < 1e-9);
    assert_eq!(h.editor.selection, Some(Selection::Opening { wall: w.id, opening: o.id }));

    let out = h.click(2.1, 0.0);
    assert!(out.edits.is_empty() && out.message.unwrap().contains("overlap"));
    h.editor.set_tool(Tool::Window);
    h.click(4.5, 0.0);
    let w = &h.data().walls[0];
    assert_eq!((w.openings.len(), w.openings[1].material.as_str(), w.openings[1].kind), (2, "glass-window", OpeningKind::Window));
    let out = h.click(3.0, 5.0);
    assert!(out.message.unwrap().contains("click on a wall"));
    // The wall material is used outside the openings and the opening material inside.
    assert_eq!(h.data().walls[0].material_at(0.05), "drywall");
    assert_eq!(h.data().walls[0].material_at(h.data().walls[0].openings[0].start + 0.01), "wood-door");
}

#[test]
fn placing_an_ap_uses_the_chosen_model_and_next_name() {
    let mut h = Harness::new(Tool::Ap);
    h.click(2.1, 3.9);
    h.click(6.0, 3.0);
    let aps = &h.data().access_points;
    assert_eq!((aps[0].name.as_str(), aps[0].position), ("AP 1", pt(2.0, 4.0)), "grid snapped");
    assert_eq!(aps[1].name, "AP 2");
    assert_eq!(aps[0].radios, h.settings.ap_model.radios);
    assert_ne!(aps[0].id, aps[1].id);
    assert_eq!(h.editor.selection, Some(Selection::Ap(aps[1].id)));
}

#[test]
fn selecting_and_moving_an_ap_is_one_undoable_edit_without_a_grab_jump() {
    let mut h = Harness::new(Tool::Ap);
    h.click(2.0, 2.0);
    h.editor.set_tool(Tool::Select);
    let before = h.history.can_undo();
    assert!(before);
    // Grab 0.1 m off-center; the AP must follow by the same offset, not jump to the cursor.
    h.ev(Pointer::Press, 2.1, 2.0);
    h.ev(Pointer::Drag, 4.1, 3.0);
    assert_eq!(h.data().access_points[0].position, pt(2.0, 2.0), "model untouched until release");
    let shown = h.editor.preview_data(h.data());
    assert_eq!(shown.access_points[0].position, pt(4.0, 3.0), "the canvas sees the AP at its new place");
    h.ev(Pointer::Release, 4.1, 3.0);
    assert_eq!(h.data().access_points[0].position, pt(4.0, 3.0));
    assert!(h.history.undo(&mut h.project));
    assert_eq!(h.data().access_points[0].position, pt(2.0, 2.0), "one undo reverts the whole move");
    assert!(h.history.undo(&mut h.project));
    assert!(h.data().access_points.is_empty());
}

#[test]
fn a_click_without_movement_selects_but_changes_nothing() {
    let mut h = Harness::new(Tool::Select).with_wall(pt(0.0, 0.0), pt(4.0, 0.0));
    let out = h.click(2.0, 0.05);
    assert!(out.edits.is_empty());
    assert_eq!(h.editor.selection, Some(Selection::Wall(h.data().walls[0].id)));
    h.click(8.0, 8.0);
    assert_eq!(h.editor.selection, None, "clicking empty space clears the selection");
}

#[test]
fn dragging_a_wall_end_reshapes_it_and_snaps_to_other_walls() {
    let mut h = Harness::new(Tool::Select).with_wall(pt(0.0, 0.0), pt(4.0, 0.0)).with_wall(pt(6.0, 0.0), pt(6.0, 5.0));
    h.drag((4.0, 0.0), (5.95, 0.05));
    let w = &h.data().walls[0];
    assert_eq!((w.a, w.b), (pt(0.0, 0.0), pt(6.0, 0.0)), "the free end snaps onto the neighbour's end");
    assert_eq!(h.data().walls[1].a, pt(6.0, 0.0), "the neighbour is untouched");
}

#[test]
fn dragging_a_wall_body_translates_it_rigidly() {
    let mut h = Harness::new(Tool::Select).with_wall(pt(0.0, 0.0), pt(4.0, 2.0));
    h.drag((2.0, 1.0), (3.0, 3.1));
    let w = &h.data().walls[0];
    assert_eq!((w.a, w.b), (pt(1.0, 2.0), pt(5.0, 4.0)), "grid-snapped start; the other end keeps the length and direction");
}

#[test]
fn collapsing_a_wall_by_dragging_its_end_is_refused() {
    let mut h = Harness::new(Tool::Select).with_wall(pt(0.0, 0.0), pt(4.0, 0.0));
    h.ev(Pointer::Press, 4.0, 0.0);
    h.ev(Pointer::Drag, 0.0, 0.0);
    let out = h.ev(Pointer::Release, 0.0, 0.0);
    assert!(out.edits.is_empty() && out.message.is_some());
    assert_eq!(h.data().walls[0].b, pt(4.0, 0.0));
}

#[test]
fn an_opening_slides_along_its_wall_but_not_through_a_neighbour() {
    let mut h = Harness::new(Tool::Door).with_wall(pt(0.0, 0.0), pt(10.0, 0.0));
    h.click(2.0, 0.0);
    h.editor.set_tool(Tool::Window);
    h.click(7.0, 0.0);
    h.editor.set_tool(Tool::Select);
    let door = h.data().walls[0].openings[0].clone();
    let center = (door.start + door.end) / 2.0;
    h.ev(Pointer::Press, 2.0, 0.0);
    h.ev(Pointer::Drag, 4.0, 0.0);
    h.ev(Pointer::Release, 4.0, 0.0);
    let o = h.data().walls[0].openings.iter().find(|o| o.id == door.id).unwrap().clone();
    assert!(((o.start + o.end) / 2.0 - (center + 0.2)).abs() < 1e-9, "moved 2 m along a 10 m wall");
    assert!(((o.end - o.start) - (door.end - door.start)).abs() < 1e-12, "width unchanged");
    // Dragging far right stops at the window instead of overlapping it.
    h.ev(Pointer::Press, 4.0, 0.0);
    h.ev(Pointer::Drag, 7.0, 0.0);
    h.ev(Pointer::Release, 7.0, 0.0);
    let w = &h.data().walls[0];
    assert!(w.openings.iter().all(|a| w.openings.iter().all(|b| a.id == b.id || a.end <= b.start || b.end <= a.start)), "no overlaps: {:?}", w.openings);
}

#[test]
fn stairwell_outline_closes_on_the_first_corner_or_enter() {
    let mut h = Harness::new(Tool::Cutout);
    for (x, y) in [(1.0, 1.0), (4.0, 1.0), (4.0, 4.0)] {
        h.click(x, y);
    }
    assert!(h.data().cutouts.is_empty() && matches!(h.editor.preview(), Preview::Polygon { can_close: true, .. }));
    h.click(1.05, 1.02);
    assert_eq!(h.data().cutouts.len(), 1);
    assert_eq!(h.data().cutouts[0].polygon, vec![pt(1.0, 1.0), pt(4.0, 1.0), pt(4.0, 4.0)]);

    for (x, y) in [(6.0, 6.0), (8.0, 6.0), (8.0, 8.0), (6.0, 8.0)] {
        h.click(x, y);
    }
    let out = h.confirm();
    assert_eq!(out.edits.len(), 1);
    assert_eq!(h.data().cutouts[1].polygon.len(), 4);
}

#[test]
fn stairwells_need_three_corners() {
    let mut h = Harness::new(Tool::Cutout);
    h.click(1.0, 1.0);
    h.click(3.0, 1.0);
    let out = h.confirm();
    assert!(out.edits.is_empty() && out.message.unwrap().contains("at least 3"));
    assert!(h.data().cutouts.is_empty());
    h.click(1.0, 1.0);
    h.editor.cancel();
    assert!(!h.editor.is_drafting());
}

#[test]
fn the_cutout_can_be_reshaped_by_its_corners() {
    let mut h = Harness::new(Tool::Cutout);
    for (x, y) in [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)] {
        h.click(x, y);
    }
    h.confirm();
    h.editor.set_tool(Tool::Select);
    h.drag((4.0, 4.0), (6.0, 5.0));
    assert_eq!(h.data().cutouts[0].polygon[2], pt(6.0, 5.0));
    h.drag((3.0, 1.0), (4.0, 2.0)); // inside the polygon: moves the whole outline
    assert_eq!(h.data().cutouts[0].polygon[0], pt(1.0, 1.0));
}

#[test]
fn delete_tool_and_key_remove_what_they_hit_and_deleting_an_opening_keeps_its_wall() {
    let mut h = Harness::new(Tool::Door).with_wall(pt(0.0, 0.0), pt(6.0, 0.0));
    h.click(3.0, 0.0);
    h.editor.set_tool(Tool::Delete);
    h.click(3.0, 0.02);
    assert_eq!((h.data().walls.len(), h.data().walls[0].openings.len()), (1, 0));
    h.click(1.0, 0.02);
    assert!(h.data().walls.is_empty());
    assert!(h.history.undo(&mut h.project) && h.history.undo(&mut h.project));
    assert_eq!(h.data().walls[0].openings.len(), 1, "both deletions undo");

    h.editor.set_tool(Tool::Select);
    h.click(1.0, 0.0);
    assert!(h.editor.selection.is_some());
    let out = h.delete_selection();
    assert_eq!(out.edits.len(), 1);
    assert!(h.data().walls.is_empty() && h.editor.selection.is_none());
    assert!(h.delete_selection().edits.is_empty(), "nothing selected, nothing to do");
}

#[test]
fn switching_tools_drops_a_half_drawn_wall() {
    let mut h = Harness::new(Tool::Wall);
    h.click(0.0, 0.0);
    assert!(h.editor.is_drafting());
    h.editor.set_tool(Tool::Select);
    assert!(!h.editor.is_drafting() && *h.editor.preview() == Preview::None);
    h.click(5.0, 5.0);
    assert!(h.data().walls.is_empty());
}

#[test]
fn selection_is_forgotten_when_its_element_disappears() {
    let mut h = Harness::new(Tool::Wall);
    h.drag((0.0, 0.0), (4.0, 0.0));
    assert!(h.editor.selection.is_some());
    assert!(h.history.undo(&mut h.project));
    h.editor.refresh_selection(&h.project.zones[0].planning);
    assert!(h.editor.selection.is_none());
}

#[test]
fn hover_shows_the_snapped_cursor_and_the_door_preview() {
    let mut h = Harness::new(Tool::Wall).with_wall(pt(0.0, 0.0), pt(6.0, 0.0));
    h.ev(Pointer::Hover, 3.04, 2.9);
    assert_eq!(*h.editor.preview(), Preview::Cursor { at: pt(3.0, 3.0), kind: SnapKind::Grid });
    h.editor.set_tool(Tool::Door);
    h.ev(Pointer::Hover, 3.0, 0.05);
    assert!(matches!(h.editor.preview(), Preview::Opening { ok: true, .. }));
    assert!(h.data().walls[0].openings.is_empty(), "hover never edits");
}
