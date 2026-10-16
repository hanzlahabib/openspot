//! Survey UI: canvas.

use eframe::egui::{self, Align2, Color32, ColorImage, FontId, PointerButton, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2};
use openspot_heatmap::{Gradient};
use crate::survey::{ZoomRequest, Calibration, HeatKey, SampleDetail, Survey, Tool, View};
use crate::survey::visibility::sample_caption;
use crate::aliases::Aliases;
use crate::ui::color_for;

pub(super) fn canvas(s: &mut Survey, ui: &mut egui::Ui, aliases: &Aliases) {
    let Some((map_w, map_h)) = s.zone().map.as_ref().map(|m| (m.width as f32, m.height as f32)) else {
        ui.centered_and_justified(|ui| {
            ui.label("Load a floor plan (PNG or JPEG) to start a survey.");
        });
        return;
    };
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    if let Some(req) = s.zoom_request.take() {
        match req {
            ZoomRequest::Fit => s.view = View::fit(vec2(map_w, map_h), rect.size()),
            ZoomRequest::Percent(pct) => {
                let factor = pct as f32 / 100.0 / s.view.zoom;
                s.view.zoom_at(rect.min, rect.center(), factor);
            }
        }
    }
    if s.needs_fit {
        s.view = View::fit(vec2(map_w, map_h), rect.size());
        s.needs_fit = false;
    }

    let moving_ap = super::ap::drag(s, &response, rect.min) || super::draw::drag(s, &response, rect.min);
    if !moving_ap && (response.dragged_by(PointerButton::Primary) || response.dragged_by(PointerButton::Middle)) {
        s.view.offset += response.drag_delta();
    }
    if response.hovered() {
        let (scroll, pinch, pointer) =
            ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
        if let Some(p) = pointer {
            let factor = crate::map_view::scroll_zoom_factor(scroll, pinch);
            if (factor - 1.0).abs() > f32::EPSILON {
                s.view.zoom_at(rect.min, p, factor);
            }
        }
    }

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let (x, y) = s.view.to_map(rect.min, pos);
            let inside = (0.0..=f64::from(map_w)).contains(&x) && (0.0..=f64::from(map_h)).contains(&y);
            if inside {
                handle_click(s, x, y);
            }
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let map_rect = Rect::from_min_size(s.view.to_screen(rect.min, (0.0, 0.0)), vec2(map_w, map_h) * s.view.zoom);
    let full_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

    if s.show_floor_plan {
        if let Some(tex) = map_texture(s, ui.ctx()) {
            painter.image(tex.id(), map_rect, full_uv, Color32::WHITE);
        }
    } else {
        painter.rect_filled(map_rect, 0.0, ui.visuals().faint_bg_color);
    }
    if s.show_heatmap {
        if let Some((tex, uv)) = heat_texture(s, ui.ctx()) {
            painter.image(tex.id(), map_rect, uv, Color32::from_white_alpha(255));
        }
    }
    super::contours::draw(s, &painter, rect.min);
    if s.show_points {
        draw_points(s, &painter, rect.min);
    }
    super::path::draw(s, &painter, rect.min);
    super::crop::draw(s, &painter, rect.min);
    super::draw::overlay(s, &painter, rect.min);
    if s.show_ap_markers {
        draw_ap_markers(s, &painter, rect.min, aliases);
    }
    draw_calibration_markers(s, &painter, rect.min);
    hint(s, &painter, rect);

    if let Some(i) = response.hover_pos().and_then(|p| nearest_point(s, rect.min, p)) {
        let point = &s.snapshot().points[i];
        let mut aps: Vec<_> = point.access_points.iter().collect();
        aps.sort_by_key(|a| std::cmp::Reverse(a.rssi_dbm));
        response.on_hover_ui_at_pointer(|ui| {
            ui.strong(format!("Point {} · {} networks", i + 1, aps.len()));
            for ap in aps.iter().take(12) {
                ui.label(format!("{} · {} dBm · ch {}", aliases.name_for(&ap.bssid, &ap.ssid), ap.rssi_dbm, ap.channel));
            }
        });
    } else if let Some(text) = response.hover_pos().and_then(|p| {
        let (x, y) = s.view.to_map(rect.min, p);
        s.heat_text_at(x, y)
    }) {
        response.on_hover_ui_at_pointer(|ui| {
            ui.label(text);
        });
    }
}

pub(super) fn handle_click(s: &mut Survey, x: f64, y: f64) {
    if s.calibration.is_active() {
        s.calibration.click((x, y));
    } else if s.tool == Tool::Measure && !s.busy {
        s.request_scan(x, y);
    } else if s.tool == Tool::Path {
        s.path_click(x, y);
    } else if s.tool == Tool::Crop {
        s.crop_click(x, y);
    } else if s.tool == Tool::Draw {
        let tolerance = super::draw::tolerance(s);
        s.draw_click(x, y, tolerance);
    }
}

pub(super) fn nearest_point(s: &Survey, origin: Pos2, cursor: Pos2) -> Option<usize> {
    s.snapshot()
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| (i, s.view.to_screen(origin, (p.x, p.y)).distance(cursor)))
        .filter(|&(_, d)| d <= HOVER_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

pub(super) fn draw_points(s: &Survey, painter: &egui::Painter, origin: Pos2) {
    let ui_text = Color32::BLACK;
    let gradient = Gradient::signal_dbm();
    for p in &s.snapshot().points {
        let best = p.access_points.iter().map(|a| a.rssi_dbm).max();
        let [r, g, b] = gradient.color(best.map_or(-100.0, |v| v as f32));
        let c = s.view.to_screen(origin, (p.x, p.y));
        painter.circle(c, POINT_RADIUS, Color32::from_rgb(r, g, b), Stroke::new(2.0, Color32::WHITE));
        if s.sample_detail == SampleDetail::Detailed {
            painter.text(c + vec2(POINT_RADIUS + 3.0, 0.0), Align2::LEFT_CENTER, sample_caption(p), FontId::proportional(11.0), ui_text);
        }
    }
}

/// Diamonds at the estimated position of each selected network, labelled with its SSID.
pub(super) fn draw_ap_markers(s: &Survey, painter: &egui::Painter, origin: Pos2, aliases: &Aliases) {
    for m in s.ap_markers() {
        let (bssid, ssid) = (m.bssid, m.ssid);
        let c = s.view.to_screen(origin, (m.x, m.y));
        let r = s.ap_marker_size.radius_px();
        let diamond = vec![c + vec2(0.0, -r), c + vec2(r, 0.0), c + vec2(0.0, r), c + vec2(-r, 0.0)];
        painter.add(egui::Shape::convex_polygon(diamond, color_for(&bssid), Stroke::new(2.0, Color32::BLACK)));
        if s.show_ap_titles {
            painter.text(
                c + vec2(r + 4.0, 0.0),
                Align2::LEFT_CENTER,
                format!("{} ({})", aliases.name_for(&bssid, &ssid), if m.manual { "manual" } else { "est." }),
                FontId::proportional(13.0),
                Color32::BLACK,
            );
        }
    }
}

pub(super) fn draw_calibration_markers(s: &Survey, painter: &egui::Painter, origin: Pos2) {
    let at = |p: (f64, f64)| s.view.to_screen(origin, p);
    let stroke = Stroke::new(2.0, Color32::from_rgb(255, 0, 200));
    match s.calibration {
        Calibration::PickSecond { a } => {
            painter.circle_stroke(at(a), 7.0, stroke);
        }
        Calibration::Enter { a, b, .. } => {
            painter.line_segment([at(a), at(b)], stroke);
            painter.circle_stroke(at(a), 7.0, stroke);
            painter.circle_stroke(at(b), 7.0, stroke);
        }
        Calibration::RectSecond { a } => {
            painter.circle_stroke(at(a), 7.0, stroke);
        }
        Calibration::RectEnter { a, b, .. } => {
            painter.rect_stroke(Rect::from_two_pos(at(a), at(b)), 0.0, stroke, egui::StrokeKind::Middle);
        }
        Calibration::Idle | Calibration::PickFirst | Calibration::RectFirst => {}
    }
}

pub(super) fn hint(s: &Survey, painter: &egui::Painter, rect: Rect) {
    let text = match (s.calibration, s.tool) {
        (Calibration::PickFirst, _) => "Calibrate: click the first point of a known distance",
        (Calibration::PickSecond { .. }, _) => "Calibrate: click the second point",
        (Calibration::Enter { .. }, _) => "Calibrate: enter the real distance",
        (Calibration::RectFirst, _) => "Calibrate: click one corner of a rectangle of known size",
        (Calibration::RectSecond { .. }, _) => "Calibrate: click the opposite corner",
        (Calibration::RectEnter { .. }, _) => "Calibrate: enter the rectangle's real size",
        (Calibration::Idle, Tool::Measure) if s.busy => "Scanning… stand still",
        (Calibration::Idle, Tool::Measure) => "Stand at a spot, then click it on the map to measure",
        (Calibration::Idle, Tool::Pan) => "Drag to pan, scroll to zoom",
        (Calibration::Idle, Tool::Draw) => "Draw: pick a tool in the bar, then click on the plan",
        (Calibration::Idle, Tool::Crop) => "Crop: click two corners (or polygon vertices) of the area to show",
        (Calibration::Idle, Tool::Path) => "Click waypoints along the route you will walk, then press Start",
    };
    painter.text(
        rect.left_top() + vec2(10.0, 8.0),
        Align2::LEFT_TOP,
        text,
        FontId::proportional(14.0),
        Color32::from_rgb(255, 0, 200),
    );
}

pub(super) fn map_texture(s: &mut Survey, ctx: &egui::Context) -> Option<TextureHandle> {
    let zone = &s.project.zones[s.zone];
    if let Some(t) = s.textures.maps.get(&zone.id) {
        return Some(t.clone());
    }
    let map = zone.map.as_ref()?;
    let color = match crate::map_view::decode_map(map) {
        Ok(c) => c,
        Err(e) => {
            s.error = Some(e);
            return None;
        }
    };
    let tex = ctx.load_texture(format!("map-{}", zone.id), color, TextureOptions::LINEAR);
    s.textures.maps.insert(zone.id, tex.clone());
    Some(tex)
}

/// Heatmap texture plus the UV rectangle covering exactly the map area (the raster may
/// be slightly larger than the map because the cell size rarely divides it evenly).
pub(super) fn heat_texture(s: &mut Survey, ctx: &egui::Context) -> Option<(TextureHandle, Rect)> {
    let key: HeatKey = s.heat_key();
    if let Some(result) = s.advance_heat() {
        // A finished background render replaces the (possibly stale) texture.
        match result {
            Some(out) => {
                let color = ColorImage::from_rgba_unmultiplied([out.grid.cols, out.grid.rows], &out.rgba);
                let tex = ctx.load_texture("heatmap", color, TextureOptions::LINEAR);
                s.textures.heat = Some((key, tex));
                s.textures.heat_grid = Some(out.grid);
                s.heat_opacity = out.alpha;
            }
            None => {
                s.textures.heat = None;
                s.textures.heat_grid = None;
                s.heat_opacity = s.opacity;
            }
        }
    } else if s.heat_progress().is_some() {
        // Keep drawing the previous texture and wake up to poll the job.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    } else if s.heat_opacity != s.opacity {
        // Opacity is baked into the pixels but needs no new interpolation: recolor the cached grid.
        if let Some(grid) = s.textures.heat_grid.as_ref() {
            let rgba = s.heat_rgba(grid);
            let color = ColorImage::from_rgba_unmultiplied([grid.cols, grid.rows], &rgba);
            let tex = ctx.load_texture("heatmap", color, TextureOptions::LINEAR);
            s.textures.heat = Some((key, tex));
        }
        s.heat_opacity = s.opacity;
    }
    let (_, tex) = s.textures.heat.as_ref()?;
    let map = s.zone().map.as_ref()?;
    let long = f64::from(map.width.max(map.height));
    let cell = long / crate::survey::HEATMAP_LONG_SIDE_CELLS;
    let [cols, rows] = tex.size();
    let u = (f64::from(map.width) / (cols as f64 * cell)) as f32;
    let v = (f64::from(map.height) / (rows as f64 * cell)) as f32;
    Some((tex.clone(), Rect::from_min_max(pos2(0.0, 0.0), pos2(u, v))))
}

const POINT_RADIUS: f32 = 6.0;

/// Screen distance within which hovering a point shows its details.
const HOVER_RADIUS: f32 = 12.0;
