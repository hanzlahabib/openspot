//! Survey UI: drawing tools bar, dialogs, selection overlay and shape dragging.

use eframe::egui::{self, Align2, Color32, Key, PointerButton, Pos2, Response, Stroke, Vec2};
use openspot_model::{FontSize, LineThickness};

use crate::survey::draw::{DrawTool, shape_bounds};
use crate::survey::{Survey, Tool};

/// Click/drag slack when picking shapes, in screen pixels (a finger-width target).
const HIT_TOLERANCE_SCREEN_PX: f64 = 6.0;
const SELECTION_COLOR: Color32 = Color32::from_rgb(0, 140, 255);
const DRAFT_COLOR: Color32 = Color32::from_rgb(255, 0, 200);
const SELECTION_MARGIN: f32 = 4.0;

/// Pick slack in map pixels at the current zoom.
pub(super) fn tolerance(s: &Survey) -> f64 {
    HIT_TOLERANCE_SCREEN_PX / f64::from(s.view.zoom.max(f32::EPSILON))
}

pub(super) fn bar(s: &mut Survey, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if s.drawing().is_none() {
            ui.label("No drawn plan in this zone. Use “Draw plan…” in the toolbar to start one.");
            return;
        }
        let mut tool = s.draw.tool;
        for (value, label) in [(DrawTool::Select, "Select"), (DrawTool::Polyline, "Line"), (DrawTool::Rect, "Rectangle"), (DrawTool::Text, "Text")] {
            ui.selectable_value(&mut tool, value, label);
        }
        s.draw.set_tool(tool);
        ui.separator();
        egui::ComboBox::from_id_salt("draw_thickness").selected_text(format!("Line: {}", s.draw.thickness.label())).show_ui(ui, |ui| {
            for t in LineThickness::ALL {
                ui.selectable_value(&mut s.draw.thickness, t, format!("{} ({} px)", t.label(), t.px()));
            }
        });
        egui::ComboBox::from_id_salt("draw_font").selected_text(format!("Text: {}", s.draw.font.label())).show_ui(ui, |ui| {
            for f in FontSize::ALL {
                ui.selectable_value(&mut s.draw.font, f, format!("{} ({} px)", f.label(), f.px()));
            }
        });
        ui.checkbox(&mut s.draw.fill_rects, "Fill rectangles");
        ui.separator();
        if s.draw.tool == DrawTool::Polyline && ui.add_enabled(s.draw.draft.len() >= 2, egui::Button::new("Finish line (Enter)")).clicked() {
            s.draw_finish_polyline();
        }
        let has_selection = s.draw.selected.is_some();
        if ui.add_enabled(has_selection, egui::Button::new("Apply style")).on_hover_text("Use the chosen thickness and font on the selected shape").clicked() {
            s.draw_restyle_selected();
        }
        if ui.add_enabled(has_selection, egui::Button::new("Edit text")).clicked() {
            if let Some(d) = s.drawing().cloned() {
                s.draw.edit_selected_text(&d);
            }
        }
        if ui.add_enabled(has_selection, egui::Button::new("Delete (Del)")).clicked() {
            s.draw_delete_selected();
        }
    });
}

pub(super) fn dialogs(s: &mut Survey, ctx: &egui::Context) {
    new_plan_dialog(s, ctx);
    text_dialog(s, ctx);
}

fn new_plan_dialog(s: &mut Survey, ctx: &egui::Context) {
    let Some((w_m, h_m)) = s.draw_dialog else { return };
    let unit = s.unit;
    let (mut w, mut h) = (unit.from_meters(w_m), unit.from_meters(h_m));
    let (mut create, mut cancel) = (false, false);
    egui::Window::new("Draw a plan").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.label("Size of the area. This replaces the zone's current plan; the scale is set automatically.");
        ui.horizontal(|ui| {
            let range = unit.from_meters(1.0)..=unit.from_meters(1000.0);
            ui.label("Width");
            ui.add(egui::DragValue::new(&mut w).speed(0.5).range(range.clone()).suffix(format!(" {}", unit.label())));
            ui.label("Height");
            ui.add(egui::DragValue::new(&mut h).speed(0.5).range(range).suffix(format!(" {}", unit.label())));
        });
        ui.horizontal(|ui| {
            create = ui.button("Create").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if create {
        match s.draw_new_plan(unit.to_meters(w), unit.to_meters(h)) {
            Ok(()) => {
                s.error = None;
                s.tool = Tool::Draw;
                s.message = Some("Drawing started. Pick a tool and click on the plan.".to_owned());
            }
            Err(e) => s.error = Some(e),
        }
        s.draw_dialog = None;
    } else if cancel {
        s.draw_dialog = None;
    } else {
        s.draw_dialog = Some((unit.to_meters(w), unit.to_meters(h)));
    }
}

fn text_dialog(s: &mut Survey, ctx: &egui::Context) {
    let Some(mut pending) = s.draw.pending_text.clone() else { return };
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new("Text").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        let field = ui.add(egui::TextEdit::multiline(&mut pending.text).desired_rows(2).desired_width(260.0));
        field.request_focus();
        ui.horizontal(|ui| {
            ok = ui.button("Place").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    s.draw.pending_text = Some(pending);
    if ok {
        s.draw_commit_text();
    } else if cancel {
        s.draw.cancel();
    }
}

/// Delete removes the selection, Enter finishes a line, Escape abandons the current action.
pub(super) fn keys(s: &mut Survey, ctx: &egui::Context) {
    if s.tool != Tool::Draw || ctx.wants_keyboard_input() {
        return;
    }
    let (delete, enter, escape) = ctx.input(|i| (i.key_pressed(Key::Delete), i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    if delete {
        s.draw_delete_selected();
    } else if enter {
        s.draw_finish_polyline();
    } else if escape {
        s.draw.cancel();
    }
}

/// Moves a shape with the pointer in Select mode. Returns whether a shape drag is active,
/// so the canvas does not pan at the same time.
pub(super) fn drag(s: &mut Survey, response: &Response, origin: Pos2) -> bool {
    if s.tool != Tool::Draw {
        return false;
    }
    if response.drag_started_by(PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            let (x, y) = s.view.to_map(origin, pos);
            let tol = tolerance(s);
            s.draw_drag_begin(x, y, tol);
        }
    }
    let active = s.draw.drag_offset().is_some();
    if active {
        if let Some(pos) = response.interact_pointer_pos() {
            s.draw.drag_update(s.view.to_map(origin, pos));
        }
        if response.drag_stopped() {
            s.draw_drag_end();
        }
    }
    active
}

/// Selection box, drag ghost and the line or rectangle being drawn.
pub(super) fn overlay(s: &Survey, painter: &egui::Painter, origin: Pos2) {
    if s.tool != Tool::Draw {
        return;
    }
    let at = |p: (f64, f64)| s.view.to_screen(origin, p);
    let Some(drawing) = s.drawing() else { return };
    if let Some(shape) = s.draw.selected.and_then(|id| drawing.get(id)) {
        if let Some((min, max)) = shape_bounds(shape) {
            let (dx, dy) = s.draw.drag_offset().map_or((0.0, 0.0), |(_, o)| o);
            let rect = egui::Rect::from_two_pos(at((min.0 + dx, min.1 + dy)), at((max.0 + dx, max.1 + dy))).expand(SELECTION_MARGIN);
            painter.rect_stroke(rect, 0.0, Stroke::new(1.5, SELECTION_COLOR), egui::StrokeKind::Middle);
        }
    }
    let draft: Vec<Pos2> = s.draw.draft.iter().map(|&p| at(p)).collect();
    if draft.len() > 1 {
        painter.add(egui::Shape::line(draft.clone(), Stroke::new(2.0, DRAFT_COLOR)));
    }
    for p in draft {
        painter.circle_filled(p, 3.5, DRAFT_COLOR);
    }
}
