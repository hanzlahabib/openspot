use image::Rgba;
use openspot_model::{Drawing, FontSize, LineThickness, Shape, ShapeKind};

use super::hit::{distance_to_segment, hit, top_hit};
use super::raster::{INK, render};
use super::session::{DrawSession, DrawTool};
use super::text::text_size;

const PAPER: Rgba<u8> = Rgba([255, 255, 255, 255]);
const INK_PX: Rgba<u8> = Rgba([INK[0], INK[1], INK[2], 255]);

fn drawing_with(shapes: Vec<Shape>) -> Drawing {
    Drawing { width: 200, height: 100, shapes }
}

fn rect(min: (f64, f64), max: (f64, f64), filled: bool) -> Shape {
    Shape::new(ShapeKind::Rect { min, max, filled }, LineThickness::Medium, FontSize::Medium)
}

fn line(points: Vec<(f64, f64)>, t: LineThickness) -> Shape {
    Shape::new(ShapeKind::Polyline { points }, t, FontSize::Medium)
}

fn text(at: (f64, f64), t: &str, f: FontSize) -> Shape {
    Shape::new(ShapeKind::Text { at, text: t.into() }, LineThickness::Medium, f)
}

// --- rasterization -------------------------------------------------------------------

#[test]
fn empty_drawing_is_white_paper_of_the_requested_size() {
    let img = render(&drawing_with(vec![])).unwrap();
    assert_eq!((img.width(), img.height()), (200, 100));
    assert!(img.pixels().all(|p| p == &PAPER));
}

#[test]
fn filled_rectangle_covers_exactly_its_area() {
    let img = render(&drawing_with(vec![rect((20.0, 30.0), (60.0, 50.0), true)])).unwrap();
    for (x, y) in [(20, 30), (40, 40), (59, 49), (21, 31)] {
        assert_eq!(img.get_pixel(x, y), &INK_PX, "inside ({x},{y})");
    }
    for (x, y) in [(19, 40), (60, 40), (40, 29), (40, 50), (0, 0), (199, 99)] {
        assert_eq!(img.get_pixel(x, y), &PAPER, "outside ({x},{y})");
    }
    let dark = img.pixels().filter(|p| p[0] < 128).count();
    assert_eq!(dark, 40 * 20, "area of the rectangle in pixels");
}

#[test]
fn outline_rectangle_is_hollow() {
    let mut r = rect((20.0, 20.0), (120.0, 80.0), false);
    r.thickness = LineThickness::Medium; // 4 px, centered on the edge
    let img = render(&drawing_with(vec![r])).unwrap();
    assert_eq!(img.get_pixel(20, 50), &INK_PX, "on the left edge");
    assert_eq!(img.get_pixel(70, 20), &INK_PX, "on the top edge");
    assert_eq!(img.get_pixel(70, 50), &PAPER, "interior stays empty");
    assert_eq!(img.get_pixel(10, 50), &PAPER, "outside");
}

#[test]
fn horizontal_line_has_the_preset_thickness_and_round_ends() {
    for (t, px) in [(LineThickness::Thin, 2), (LineThickness::Medium, 4), (LineThickness::Thick, 8)] {
        let img = render(&drawing_with(vec![line(vec![(50.0, 50.0), (150.0, 50.0)], t)])).unwrap();
        let rows: Vec<u32> = (0..100).filter(|y| img.get_pixel(100, *y)[0] < 128).collect();
        assert_eq!(rows.len(), px, "{t:?} column in the middle: {rows:?}");
        assert!(rows.contains(&49) && rows.contains(&50) || px == 2 && rows == [49, 50]);
        // Endpoints are painted, and with a round cap the line extends half a width past them.
        assert!(img.get_pixel(50, 50)[0] < 128 && img.get_pixel(149, 50)[0] < 128);
        assert_eq!(img.get_pixel(50 - px as u32, 50), &PAPER, "no extension beyond the cap radius ({t:?})");
        assert_eq!(img.get_pixel(20, 50), &PAPER);
    }
}

#[test]
fn polyline_follows_its_corners() {
    let img = render(&drawing_with(vec![line(vec![(20.0, 20.0), (100.0, 20.0), (100.0, 80.0)], LineThickness::Medium)])).unwrap();
    assert!(img.get_pixel(60, 20)[0] < 128, "first segment");
    assert!(img.get_pixel(100, 50)[0] < 128, "second segment");
    assert_eq!(img.get_pixel(60, 50), &PAPER, "not the chord between the ends");
}

#[test]
fn later_shapes_paint_over_earlier_ones() {
    // Filled black over a line; a white-paper pixel stays white where nothing is drawn.
    let d = drawing_with(vec![line(vec![(0.0, 40.0), (200.0, 40.0)], LineThickness::Thin), rect((90.0, 30.0), (110.0, 50.0), true)]);
    let img = render(&d).unwrap();
    assert_eq!(img.get_pixel(100, 40), &INK_PX);
}

#[test]
fn text_is_drawn_inside_its_measured_box_and_nowhere_else() {
    let (w, h) = text_size("Hello", 24.0).unwrap();
    assert!(w > 30.0 && h > 20.0 && h < 40.0, "{w} x {h}");
    let img = render(&drawing_with(vec![text((20.0, 30.0), "Hello", FontSize::Medium)])).unwrap();
    let dark: Vec<(u32, u32)> = img.enumerate_pixels().filter(|(_, _, p)| p[0] < 200).map(|(x, y, _)| (x, y)).collect();
    assert!(dark.len() > 100, "glyphs were painted");
    for (x, y) in dark {
        assert!(f64::from(x) >= 19.0 && f64::from(x) <= 20.0 + w + 1.0, "x={x}");
        assert!(f64::from(y) >= 29.0 && f64::from(y) <= 30.0 + h + 1.0, "y={y}");
    }
}

#[test]
fn bigger_font_presets_make_bigger_text_and_more_ink() {
    let sizes: Vec<(f64, f64)> = FontSize::ALL.iter().map(|f| text_size("Wall", f.px()).unwrap()).collect();
    assert!(sizes.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1));
    let ink = |f| {
        let d = Drawing { width: 400, height: 200, shapes: vec![text((5.0, 5.0), "Wall", f)] };
        render(&d).unwrap().pixels().filter(|p| p[0] < 128).count()
    };
    assert!(ink(FontSize::Small) < ink(FontSize::Medium) && ink(FontSize::Medium) < ink(FontSize::Large));
}

#[test]
fn multi_line_text_is_taller_and_blank_text_draws_nothing() {
    let one = text_size("ab", 24.0).unwrap();
    let two = text_size("ab\nab", 24.0).unwrap();
    assert!((two.1 - 2.0 * one.1).abs() < 0.01);
    assert!((two.0 - one.0).abs() < 0.01);
    let img = render(&drawing_with(vec![text((10.0, 10.0), "", FontSize::Medium)])).unwrap();
    assert!(img.pixels().all(|p| p == &PAPER));
}

#[test]
fn shapes_partly_or_wholly_off_canvas_do_not_panic() {
    let d = drawing_with(vec![
        rect((-50.0, -50.0), (20.0, 20.0), true),
        line(vec![(-100.0, 50.0), (500.0, 50.0)], LineThickness::Thick),
        text((190.0, 90.0), "clipped", FontSize::Huge),
        text((-500.0, -500.0), "gone", FontSize::Small),
    ]);
    let img = render(&d).unwrap();
    assert_eq!(img.get_pixel(5, 5), &INK_PX);
}

#[test]
fn invalid_sizes_are_rejected() {
    assert!(render(&Drawing::new(0, 10)).is_err());
    assert!(render(&Drawing::new(10, super::MAX_DRAWING_SIDE_PX + 1)).is_err());
}

#[test]
fn png_round_trips_to_the_same_pixels() {
    let d = drawing_with(vec![rect((10.0, 10.0), (30.0, 30.0), true)]);
    let png = super::render_png(&d).unwrap();
    let back = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(back, render(&d).unwrap());
}

// --- hit testing ---------------------------------------------------------------------

#[test]
fn segment_distance_handles_ends_and_degenerate_segments() {
    assert_eq!(distance_to_segment((5.0, 3.0), (0.0, 0.0), (10.0, 0.0)), 3.0);
    assert_eq!(distance_to_segment((-4.0, 3.0), (0.0, 0.0), (10.0, 0.0)), 5.0);
    assert_eq!(distance_to_segment((13.0, 4.0), (0.0, 0.0), (10.0, 0.0)), 5.0);
    assert_eq!(distance_to_segment((3.0, 4.0), (0.0, 0.0), (0.0, 0.0)), 5.0);
}

#[test]
fn polyline_hits_within_half_thickness_plus_tolerance() {
    let l = line(vec![(0.0, 0.0), (100.0, 0.0)], LineThickness::Thick); // half width 4
    assert!(hit(&l, (50.0, 3.9), 0.0));
    assert!(!hit(&l, (50.0, 4.1), 0.0));
    assert!(hit(&l, (50.0, 9.0), 6.0));
    assert!(!hit(&l, (50.0, 11.0), 6.0));
    assert!(!hit(&line(vec![], LineThickness::Thin), (0.0, 0.0), 5.0));
}

#[test]
fn outline_rect_hits_on_its_edge_but_filled_rect_hits_inside() {
    let outline = rect((10.0, 10.0), (90.0, 70.0), false);
    assert!(hit(&outline, (10.0, 40.0), 0.0));
    assert!(hit(&outline, (50.0, 11.0), 0.0));
    assert!(!hit(&outline, (50.0, 40.0), 0.0), "empty interior");
    let filled = rect((10.0, 10.0), (90.0, 70.0), true);
    assert!(hit(&filled, (50.0, 40.0), 0.0));
    assert!(!hit(&filled, (95.0, 40.0), 1.0));
    assert!(hit(&filled, (95.0, 40.0), 4.0));
}

#[test]
fn text_hits_inside_its_box() {
    let t = text((50.0, 20.0), "Room 12", FontSize::Large);
    let (w, h) = text_size("Room 12", 36.0).unwrap();
    assert!(hit(&t, (50.0 + w / 2.0, 20.0 + h / 2.0), 0.0));
    assert!(!hit(&t, (50.0 + w + 5.0, 20.0 + h / 2.0), 0.0));
    assert!(!hit(&t, (49.0, 19.0), 0.0));
}

#[test]
fn topmost_shape_wins_and_misses_return_none() {
    let below = rect((0.0, 0.0), (100.0, 100.0), true);
    let above = rect((40.0, 40.0), (60.0, 60.0), true);
    let (below_id, above_id) = (below.id, above.id);
    let d = drawing_with(vec![below, above]);
    assert_eq!(top_hit(&d, (50.0, 50.0), 0.0), Some(above_id));
    assert_eq!(top_hit(&d, (10.0, 10.0), 0.0), Some(below_id));
    assert_eq!(top_hit(&d, (150.0, 150.0), 0.0), None);
}

fn session(f: impl FnOnce(&mut DrawSession)) -> DrawSession {
    let mut s = DrawSession::default();
    f(&mut s);
    s
}

// --- editing session -----------------------------------------------------------------

#[test]
fn rectangle_tool_creates_a_normalised_shape_with_the_chosen_style() {
    let mut s = session(|s| { s.tool = DrawTool::Rect; s.thickness = LineThickness::Thick; s.fill_rects = true; });
    let mut d = Drawing::new(200, 100);
    assert!(!s.click(&mut d, (80.0, 70.0), 2.0));
    assert!(s.click(&mut d, (20.0, 10.0), 2.0));
    assert_eq!(d.shapes.len(), 1);
    assert_eq!(d.shapes[0].kind, ShapeKind::Rect { min: (20.0, 10.0), max: (80.0, 70.0), filled: true });
    assert_eq!(d.shapes[0].thickness, LineThickness::Thick);
    assert_eq!(s.selected, Some(d.shapes[0].id));
    assert!(s.draft.is_empty());
}

#[test]
fn zero_area_rectangle_is_not_created() {
    let mut s = session(|s| { s.tool = DrawTool::Rect; });
    let mut d = Drawing::new(200, 100);
    s.click(&mut d, (5.0, 5.0), 2.0);
    assert!(!s.click(&mut d, (5.0, 5.0), 2.0));
    assert!(d.shapes.is_empty());
}

#[test]
fn polyline_needs_two_points_and_enter_finishes_it() {
    let mut s = session(|s| { s.tool = DrawTool::Polyline; });
    let mut d = Drawing::new(200, 100);
    s.click(&mut d, (0.0, 0.0), 2.0);
    assert!(!s.finish_polyline(&mut d));
    s.click(&mut d, (10.0, 0.0), 2.0);
    s.click(&mut d, (10.0, 10.0), 2.0);
    assert!(s.finish_polyline(&mut d));
    assert_eq!(d.shapes[0].kind, ShapeKind::Polyline { points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)] });
    assert!(s.draft.is_empty() && !s.finish_polyline(&mut d));
}

#[test]
fn switching_tool_or_cancelling_drops_the_draft() {
    let mut s = session(|s| { s.tool = DrawTool::Polyline; });
    let mut d = Drawing::new(200, 100);
    s.click(&mut d, (0.0, 0.0), 2.0);
    s.set_tool(DrawTool::Rect);
    assert!(s.draft.is_empty());
    s.click(&mut d, (0.0, 0.0), 2.0);
    s.cancel();
    assert!(s.draft.is_empty());
}

#[test]
fn text_tool_collects_text_then_places_it_and_ignores_blank_input() {
    let mut s = session(|s| { s.tool = DrawTool::Text; s.font = FontSize::Large; });
    let mut d = Drawing::new(200, 100);
    assert!(!s.click(&mut d, (30.0, 40.0), 2.0));
    assert!(!s.commit_text(&mut d), "nothing typed yet");
    assert!(d.shapes.is_empty());
    s.click(&mut d, (30.0, 40.0), 2.0);
    s.pending_text.as_mut().unwrap().text = "  Server room \n".into();
    assert!(s.commit_text(&mut d));
    assert_eq!(d.shapes[0].kind, ShapeKind::Text { at: (30.0, 40.0), text: "  Server room".into() });
    assert_eq!(d.shapes[0].font, FontSize::Large);
}

#[test]
fn editing_a_text_shape_changes_only_its_content() {
    let mut s = DrawSession::default();
    let t = text((5.0, 6.0), "old", FontSize::Small);
    let id = t.id;
    let mut d = drawing_with(vec![t]);
    s.selected = Some(id);
    assert!(s.edit_selected_text(&d));
    s.pending_text.as_mut().unwrap().text = "new".into();
    assert!(s.commit_text(&mut d));
    assert_eq!(d.shapes[0].kind, ShapeKind::Text { at: (5.0, 6.0), text: "new".into() });
    assert_eq!(d.shapes[0].id, id);
    // Selecting a non-text shape cannot be edited as text.
    let r = rect((0.0, 0.0), (5.0, 5.0), true);
    s.selected = Some(r.id);
    d.shapes.push(r);
    assert!(!s.edit_selected_text(&d));
}

#[test]
fn select_picks_the_shape_under_the_click_and_clears_on_a_miss() {
    let mut s = DrawSession::default();
    let r = rect((10.0, 10.0), (50.0, 50.0), true);
    let id = r.id;
    let mut d = drawing_with(vec![r]);
    assert!(!s.click(&mut d, (30.0, 30.0), 2.0), "selecting changes nothing");
    assert_eq!(s.selected, Some(id));
    s.click(&mut d, (150.0, 90.0), 2.0);
    assert_eq!(s.selected, None);
}

#[test]
fn dragging_moves_the_shape_only_when_dropped() {
    let mut s = DrawSession::default();
    let r = rect((10.0, 10.0), (50.0, 50.0), true);
    let id = r.id;
    let mut d = drawing_with(vec![r]);
    assert!(!s.drag_begin(&d, (150.0, 90.0), 2.0), "nothing under the pointer");
    assert!(s.drag_begin(&d, (30.0, 30.0), 2.0));
    s.drag_update((45.0, 20.0));
    assert_eq!(s.drag_offset(), Some((id, (15.0, -10.0))));
    assert_eq!(d.shapes[0].kind, ShapeKind::Rect { min: (10.0, 10.0), max: (50.0, 50.0), filled: true }, "unchanged mid-drag");
    assert!(s.drag_end(&mut d));
    assert_eq!(d.shapes[0].kind, ShapeKind::Rect { min: (25.0, 0.0), max: (65.0, 40.0), filled: true });
    assert_eq!(d.shapes[0].id, id);
    assert!(s.drag_offset().is_none());
}

#[test]
fn a_drag_that_ends_where_it_began_is_not_a_change_and_other_tools_cannot_drag() {
    let mut s = DrawSession::default();
    let mut d = drawing_with(vec![rect((10.0, 10.0), (50.0, 50.0), true)]);
    s.drag_begin(&d, (30.0, 30.0), 2.0);
    s.drag_update((40.0, 40.0));
    s.drag_update((30.0, 30.0));
    assert!(!s.drag_end(&mut d));
    s.set_tool(DrawTool::Polyline);
    assert!(!s.drag_begin(&d, (30.0, 30.0), 2.0));
}

#[test]
fn delete_removes_only_the_selected_shape() {
    let mut s = DrawSession::default();
    let keep = rect((0.0, 0.0), (5.0, 5.0), true);
    let gone = rect((50.0, 50.0), (60.0, 60.0), true);
    let (keep_id, gone_id) = (keep.id, gone.id);
    let mut d = drawing_with(vec![keep, gone]);
    assert!(!s.delete_selected(&mut d));
    s.selected = Some(gone_id);
    assert!(s.delete_selected(&mut d));
    assert_eq!(d.shapes.iter().map(|x| x.id).collect::<Vec<_>>(), [keep_id]);
    assert_eq!(s.selected, None);
}

#[test]
fn restyle_applies_the_presets_to_the_selection() {
    let mut s = session(|s| { s.thickness = LineThickness::Thick; s.font = FontSize::Huge; });
    let r = rect((0.0, 0.0), (5.0, 5.0), true);
    s.selected = Some(r.id);
    let mut d = drawing_with(vec![r]);
    assert!(s.restyle_selected(&mut d));
    assert_eq!((d.shapes[0].thickness, d.shapes[0].font), (LineThickness::Thick, FontSize::Huge));
    assert!(!s.restyle_selected(&mut d), "already that style");
}
