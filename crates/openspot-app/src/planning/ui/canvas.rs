//! The planning canvas: floor plan, plan drawing and mouse input.
//! UNVERIFIED: written without a display. The state machines it drives and the transform it
//! uses are unit-tested; the event wiring below is not.

use eframe::egui::{self, Color32, PointerButton, Rect, Sense, TextureHandle, TextureOptions, pos2, vec2};
use openspot_model::Uuid;

use super::draw;
use super::frame::Frame;
use crate::planning::Planning;
use crate::planning::levels::LevelStack;
use crate::planning::tools::{Pointer, Tool};
use crate::survey::{Survey, View};

/// A decoded floor plan on the GPU, tied to the image it was made from.
pub(crate) struct MapTexture {
    /// Byte length and size of the source image; any change means the plan was replaced.
    key: (usize, u32, u32),
    handle: TextureHandle,
}

fn map_texture(p: &mut Planning, s: &mut Survey, ctx: &egui::Context) -> Option<TextureHandle> {
    let zone = s.project.zones.get(s.zone)?;
    let map = zone.map.as_ref()?;
    let key = (map.data.len(), map.width, map.height);
    let id: Uuid = zone.id;
    if let Some(t) = p.textures.get(&id).filter(|t| t.key == key) {
        return Some(t.handle.clone());
    }
    match crate::map_view::decode_map(map) {
        Ok(color) => {
            let handle = ctx.load_texture(format!("plan-map-{id}"), color, TextureOptions::LINEAR);
            p.textures.insert(id, MapTexture { key, handle: handle.clone() });
            Some(handle)
        }
        Err(e) => {
            p.notice = Some(e);
            None
        }
    }
}

pub(super) fn canvas(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    let Some((map_w, map_h)) = s.zone().map.as_ref().map(|m| (m.width as f32, m.height as f32)) else {
        super::setup::no_plan(p, s, ui);
        return;
    };
    let scale = s.zone().scale;
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    if p.needs_fit || std::mem::take(&mut p.zoom_fit_requested) {
        p.view = View::fit(vec2(map_w, map_h), rect.size());
        p.needs_fit = false;
    }

    // Pan with the middle button, or Space + primary drag; zoom with the wheel.
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    let panning_keys = response.dragged_by(PointerButton::Middle) || (space && response.dragged_by(PointerButton::Primary));
    if panning_keys {
        p.view.offset += response.drag_delta();
    }
    if response.hovered() {
        let (scroll, pinch, pointer) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
        if let Some(at) = pointer {
            let factor = crate::map_view::scroll_zoom_factor(scroll, pinch);
            if (factor - 1.0).abs() > f32::EPSILON {
                p.view.zoom_at(rect.min, at, factor);
            }
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let map_rect = Rect::from_min_size(p.view.to_screen(rect.min, (0.0, 0.0)), vec2(map_w, map_h) * p.view.zoom);
    if let Some(tex) = map_texture(p, s, ui.ctx()) {
        painter.image(tex.id(), map_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }

    let Some(scale) = scale else {
        painter.text(rect.left_top() + vec2(10.0, 8.0), egui::Align2::LEFT_TOP, "Set the plan's scale to draw (see the banner above).", egui::FontId::proportional(14.0), Color32::from_rgb(255, 0, 200));
        return;
    };
    let frame = Frame { origin: rect.min, view: p.view, pixels_per_meter: scale.pixels_per_meter() };
    if p.show_prediction
        && let Some((tex, uv)) = heat_texture(p, ui.ctx(), map_w, map_h)
    {
        painter.image(tex.id(), map_rect, uv, Color32::WHITE);
    }

    if !panning_keys {
        drive_tools(p, s, &response, ui, &frame, space);
    }

    // Draw the plan as the tool will leave it (mid-drag included).
    let zone_index = s.zone;
    if p.show_floor_below
        && let Some(below) = LevelStack::neighbor(&s.project, zone_index, -1)
    {
        draw::ghost(&painter, &frame, &s.project.zones[below].planning);
    }
    let data = p.editor.preview_data(&s.project.zones[zone_index].planning);
    draw::plan(&painter, &frame, &data, p.editor.selection);
    draw::suggestions(&painter, &frame, p.suggestions_for(s.project.zones[zone_index].id));
    draw::preview(&painter, &frame, p.editor.preview(), &data, s.unit);
    painter.text(rect.left_top() + vec2(10.0, 8.0), egui::Align2::LEFT_TOP, p.editor.tool().hint(), egui::FontId::proportional(14.0), Color32::from_rgb(255, 0, 200));

    if p.show_prediction
        && !p.editor.is_drafting()
        && let Some(pos) = response.hover_pos()
    {
        let (x, y) = p.view.to_map(rect.min, pos);
        if let Some(text) = p.heat_image().and_then(|img| img.text_at(x, y)) {
            response.on_hover_ui_at_pointer(|ui| {
                ui.label(text);
            });
        }
    }
}

/// Texture of the shown prediction view plus the UV rectangle covering exactly the plan (the
/// raster is a whole number of cells, slightly larger than the plan).
fn heat_texture(p: &mut Planning, ctx: &egui::Context, map_w: f32, map_h: f32) -> Option<(TextureHandle, Rect)> {
    p.heat_image()?;
    let cache = p.heat.as_mut()?;
    let image = cache.image.as_ref()?;
    let (cols, rows, cell) = (image.grid.cols, image.grid.rows, image.grid.cell_px);
    if cache.texture.is_none() {
        let color = egui::ColorImage::from_rgba_unmultiplied([cols, rows], &image.rgba);
        cache.texture = Some(ctx.load_texture("planning-heat", color, TextureOptions::LINEAR));
    }
    let u = (f64::from(map_w) / (cols as f64 * cell)) as f32;
    let v = (f64::from(map_h) / (rows as f64 * cell)) as f32;
    Some((cache.texture.clone()?, Rect::from_min_max(pos2(0.0, 0.0), pos2(u, v))))
}

/// Translates egui pointer state into tool events.
fn drive_tools(p: &mut Planning, s: &mut Survey, response: &egui::Response, ui: &egui::Ui, frame: &Frame, space: bool) {
    let zone = s.zone;
    let tol = frame.tolerance_m();
    let at = |pos: egui::Pos2| frame.to_plan(pos);

    if response.secondary_clicked() {
        p.editor.cancel();
        return;
    }
    if space {
        return;
    }
    if response.drag_started_by(PointerButton::Primary) {
        let press = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
        if let Some(pos) = press {
            p.pointer(&mut s.project, zone, Pointer::Press, at(pos), tol);
            // Dragging empty space with the Select tool pans, like grabbing the paper.
            p.pan_drag = p.editor.tool() == Tool::Select && !p.editor.is_grabbing();
        }
    }
    if response.dragged_by(PointerButton::Primary) {
        if p.pan_drag {
            p.view.offset += response.drag_delta();
        } else if let Some(pos) = response.interact_pointer_pos() {
            p.pointer(&mut s.project, zone, Pointer::Drag, at(pos), tol);
        }
    }
    if response.drag_stopped_by(PointerButton::Primary) {
        if std::mem::take(&mut p.pan_drag) {
            return;
        }
        if let Some(pos) = response.interact_pointer_pos() {
            p.pointer(&mut s.project, zone, Pointer::Release, at(pos), tol);
        }
    }
    if response.clicked_by(PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        p.pointer(&mut s.project, zone, Pointer::Press, at(pos), tol);
        p.pointer(&mut s.project, zone, Pointer::Release, at(pos), tol);
    }
    let button_down = ui.input(|i| i.pointer.any_down());
    if !button_down
        && let Some(pos) = response.hover_pos()
    {
        p.pointer(&mut s.project, zone, Pointer::Hover, at(pos), tol);
    }
}
