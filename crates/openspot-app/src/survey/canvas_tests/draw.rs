use super::*;
use crate::survey::draw::DrawTool;
use openspot_model::ShapeKind;

fn map_pixels(s: &Survey) -> image::RgbaImage {
    image::load_from_memory(&s.zone().map.as_ref().unwrap().data).unwrap().to_rgba8()
}

/// A 20 x 10 m drawn plan: 1600 x 800 px at 80 px/m.
fn survey() -> Survey {
    let mut s = Survey::default();
    s.draw_new_plan(20.0, 10.0).unwrap();
    s
}

#[test]
fn new_plan_sets_map_size_scale_and_an_empty_drawing() {
    let s = survey();
    let map = s.zone().map.as_ref().unwrap();
    assert_eq!((map.width, map.height), (1600, 800));
    assert_eq!(s.zone().scale.unwrap().pixels_per_meter(), 80.0);
    assert!(s.drawing().unwrap().shapes.is_empty());
    assert!(map_pixels(&s).pixels().all(|p| p.0 == [255, 255, 255, 255]));
    assert!(s.dirty);
}

#[test]
fn bad_sizes_are_rejected_without_touching_the_zone() {
    let mut s = Survey::default();
    assert!(s.draw_new_plan(0.5, 10.0).is_err());
    assert!(s.draw_new_plan(f64::NAN, 10.0).is_err());
    assert!(s.draw_new_plan(20.0, 5000.0).is_err());
    assert!(s.zone().map.is_none() && s.drawing().is_none());
}

#[test]
fn clicking_with_the_rectangle_tool_updates_the_zone_map_image() {
    let mut s = survey();
    s.draw.set_tool(DrawTool::Rect);
    s.draw.fill_rects = true;
    s.draw_click(100.0, 100.0, 5.0);
    assert!(map_pixels(&s).pixels().all(|p| p.0 == [255, 255, 255, 255]), "nothing drawn until the second corner");
    s.draw_click(300.0, 200.0, 5.0);
    let img = map_pixels(&s);
    assert_eq!(img.get_pixel(200, 150)[0], 30, "inside the new rectangle");
    assert_eq!(img.get_pixel(400, 150)[0], 255);
    assert_eq!(s.drawing().unwrap().shapes.len(), 1);
}

#[test]
fn moving_a_shape_moves_its_pixels() {
    let mut s = survey();
    s.draw.set_tool(DrawTool::Rect);
    s.draw.fill_rects = true;
    s.draw_click(100.0, 100.0, 5.0);
    s.draw_click(200.0, 200.0, 5.0);
    s.draw.set_tool(DrawTool::Select);
    assert!(s.draw_drag_begin(150.0, 150.0, 5.0));
    s.draw.drag_update((650.0, 150.0));
    assert_eq!(map_pixels(&s).get_pixel(150, 150)[0], 30, "the plan only changes on drop");
    s.draw_drag_end();
    let img = map_pixels(&s);
    assert_eq!(img.get_pixel(150, 150)[0], 255, "old place is empty");
    assert_eq!(img.get_pixel(650, 150)[0], 30, "new place is painted");
}

#[test]
fn deleting_a_shape_erases_it_from_the_plan() {
    let mut s = survey();
    s.draw.set_tool(DrawTool::Rect);
    s.draw.fill_rects = true;
    s.draw_click(100.0, 100.0, 5.0);
    s.draw_click(200.0, 200.0, 5.0);
    s.draw_delete_selected();
    assert!(s.drawing().unwrap().shapes.is_empty());
    assert!(map_pixels(&s).pixels().all(|p| p.0 == [255, 255, 255, 255]));
}

#[test]
fn text_is_placed_through_the_pending_dialog_state() {
    let mut s = survey();
    s.draw.set_tool(DrawTool::Text);
    s.draw_click(50.0, 60.0, 5.0);
    s.draw.pending_text.as_mut().unwrap().text = "Lobby".into();
    s.draw_commit_text();
    let shape = &s.drawing().unwrap().shapes[0];
    assert_eq!(shape.kind, ShapeKind::Text { at: (50.0, 60.0), text: "Lobby".into() });
    assert!(map_pixels(&s).pixels().any(|p| p[0] < 128), "glyphs rendered into the map");
}

#[test]
fn drawing_and_rendered_map_survive_save_and_open_and_stay_editable() {
    let mut s = survey();
    s.draw.set_tool(DrawTool::Rect);
    s.draw.fill_rects = true;
    s.draw_click(100.0, 100.0, 5.0);
    s.draw_click(200.0, 200.0, 5.0);
    let dir = std::env::temp_dir().join(format!("openspot-draw-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    s.save_to(&dir.join("d.ospot")).unwrap();
    let mut t = Survey::default();
    t.open_from(&dir.join("d.ospot")).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    assert_eq!(t.drawing(), s.drawing());
    assert_eq!(map_pixels(&t), map_pixels(&s));
    // Still editable after reload: delete the loaded shape.
    t.draw.selected = Some(t.drawing().unwrap().shapes[0].id);
    t.draw_delete_selected();
    assert!(map_pixels(&t).pixels().all(|p| p.0 == [255, 255, 255, 255]));
}

#[test]
fn measurements_made_on_a_drawn_plan_are_unaffected_by_later_edits() {
    let mut s = survey();
    s.add_scan(400.0, 300.0, vec![ap(1, -50)]);
    s.draw.set_tool(DrawTool::Rect);
    s.draw_click(10.0, 10.0, 5.0);
    s.draw_click(60.0, 60.0, 5.0);
    assert_eq!(s.snapshot().points.len(), 1);
    assert_eq!(s.zone().scale.unwrap().pixels_per_meter(), 80.0);
}

#[test]
fn loading_a_real_plan_or_a_blank_grid_drops_the_drawing() {
    let mut s = survey();
    s.create_blank_map(10.0, 10.0).unwrap();
    assert!(s.drawing().is_none());
    let mut t = survey();
    let dir = std::env::temp_dir().join(format!("openspot-draw-load-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("plan.png");
    image::RgbaImage::from_pixel(40, 30, image::Rgba([1, 2, 3, 255])).save(&file).unwrap();
    t.load_map(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    assert!(t.drawing().is_none());
    assert_eq!(t.zone().map.as_ref().unwrap().width, 40);
}

#[test]
fn drawings_are_per_zone() {
    let mut s = survey();
    s.add_zone();
    assert!(s.drawing().is_none());
    s.select_zone(0);
    assert!(s.drawing().is_some());
}

#[test]
fn clicks_without_a_drawing_do_nothing() {
    let mut s = Survey::default();
    s.draw_click(1.0, 1.0, 5.0);
    s.draw_finish_polyline();
    assert!(s.zone().map.is_none() && s.error.is_none());
}
