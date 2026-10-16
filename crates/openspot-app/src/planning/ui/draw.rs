//! Painting of the plan: walls, openings, stairwells, access points and tool previews.
//! UNVERIFIED visually: written without a display; the geometry it consumes is tested.

use eframe::egui::{Align2, Color32, FontId, Painter, Shape, Stroke, pos2, vec2};
use openspot_model::{LengthUnit, OpeningKind, PlanningData, Point, VirtualAp, Wall, WallKind};

use super::frame::Frame;
use crate::planning::geometry::SnapKind;
use crate::planning::ops::Selection;
use crate::planning::tools::Preview;

const WALL: Color32 = Color32::from_rgb(55, 60, 75);
const SHELF: Color32 = Color32::from_rgb(205, 120, 30);
const DOOR: Color32 = Color32::from_rgb(150, 90, 40);
const WINDOW: Color32 = Color32::from_rgb(70, 165, 215);
const AP: Color32 = Color32::from_rgb(30, 110, 235);
const AP_OFF: Color32 = Color32::from_rgb(130, 130, 140);
const CUTOUT: Color32 = Color32::from_rgb(200, 60, 160);
const SELECTED: Color32 = Color32::from_rgb(255, 205, 40);
const OK: Color32 = Color32::from_rgb(40, 170, 80);
const BAD: Color32 = Color32::from_rgb(220, 60, 50);

/// Thinnest line a wall is drawn with, so far-zoomed-out plans stay visible.
const MIN_WALL_PX: f32 = 2.0;
const AP_RADIUS_PX: f32 = 8.0;
const VERTEX_RADIUS_PX: f32 = 4.0;
/// Extra width of the highlight under a selected wall.
const SELECTION_PAD_PX: f32 = 5.0;

pub(super) fn wall_color(kind: WallKind) -> Color32 {
    match kind {
        WallKind::Wall => WALL,
        WallKind::Shelf => SHELF,
        WallKind::Door => DOOR,
        WallKind::Window => WINDOW,
    }
}

fn opening_color(kind: OpeningKind) -> Color32 {
    match kind {
        OpeningKind::Door => DOOR,
        OpeningKind::Window => WINDOW,
    }
}

fn along(w: &Wall, u: f64) -> Point {
    Point::new(w.a.x + (w.b.x - w.a.x) * u, w.a.y + (w.b.y - w.a.y) * u)
}

fn wall_width(f: &Frame, w: &Wall) -> f32 {
    f.length_px(w.thickness_m).max(MIN_WALL_PX)
}

/// Proposed access-point positions: numbered dashed rings.
pub(super) fn suggestions(painter: &Painter, f: &Frame, list: &[crate::planning::suggest::Proposal]) {
    for (i, sug) in list.iter().enumerate() {
        let c = f.to_screen(sug.position);
        painter.circle_filled(c, AP_RADIUS_PX + 3.0, OK.gamma_multiply(0.25));
        painter.circle_stroke(c, AP_RADIUS_PX + 3.0, Stroke::new(2.0, OK));
        painter.text(c, Align2::CENTER_CENTER, (i + 1).to_string(), FontId::proportional(13.0), Color32::BLACK);
    }
}

/// A neighbouring floor's walls and stairwells, faint, to line up stairwells and shafts.
pub(super) fn ghost(painter: &Painter, f: &Frame, data: &PlanningData) {
    let color = Color32::from_gray(120).gamma_multiply(0.45);
    for c in &data.cutouts {
        painter.add(Shape::closed_line(c.polygon.iter().map(|p| f.to_screen(*p)).collect(), Stroke::new(1.5, color)));
    }
    for w in &data.walls {
        painter.line_segment([f.to_screen(w.a), f.to_screen(w.b)], Stroke::new(wall_width(f, w).max(1.5), color));
    }
}

/// Everything stored in the plan, under the editing previews.
pub(super) fn plan(painter: &Painter, f: &Frame, data: &PlanningData, selection: Option<Selection>) {
    for c in &data.cutouts {
        let pts: Vec<_> = c.polygon.iter().map(|p| f.to_screen(*p)).collect();
        let selected = selection == Some(Selection::Cutout(c.id));
        painter.add(Shape::convex_polygon(pts.clone(), CUTOUT.gamma_multiply(0.25), Stroke::NONE));
        painter.add(Shape::closed_line(pts.clone(), Stroke::new(if selected { 3.0 } else { 1.5 }, if selected { SELECTED } else { CUTOUT })));
        if selected {
            for p in pts {
                painter.circle_filled(p, VERTEX_RADIUS_PX, SELECTED);
            }
        }
    }
    for w in &data.walls {
        let (a, b) = (f.to_screen(w.a), f.to_screen(w.b));
        let width = wall_width(f, w);
        if selection == Some(Selection::Wall(w.id)) {
            painter.line_segment([a, b], Stroke::new(width + SELECTION_PAD_PX, SELECTED));
        }
        painter.line_segment([a, b], Stroke::new(width, wall_color(w.kind)));
        for o in &w.openings {
            let (oa, ob) = (f.to_screen(along(w, o.start)), f.to_screen(along(w, o.end)));
            if selection == Some(Selection::Opening { wall: w.id, opening: o.id }) {
                painter.line_segment([oa, ob], Stroke::new(width + SELECTION_PAD_PX, SELECTED));
            }
            painter.line_segment([oa, ob], Stroke::new(width, opening_color(o.kind)));
        }
        if selection == Some(Selection::Wall(w.id)) {
            for p in [a, b] {
                painter.circle_filled(p, VERTEX_RADIUS_PX, SELECTED);
            }
        }
    }
    for ap in &data.access_points {
        ap_marker(painter, f, ap, selection == Some(Selection::Ap(ap.id)));
    }
}

fn ap_marker(painter: &Painter, f: &Frame, ap: &VirtualAp, selected: bool) {
    let c = f.to_screen(ap.position);
    let on = ap.radios.iter().any(|r| r.enabled);
    if selected {
        painter.circle_filled(c, AP_RADIUS_PX + 4.0, SELECTED);
    }
    painter.circle(c, AP_RADIUS_PX, if on { AP } else { AP_OFF }, Stroke::new(2.0, Color32::WHITE));
    painter.text(c + vec2(0.0, AP_RADIUS_PX + 3.0), Align2::CENTER_TOP, &ap.name, FontId::proportional(12.0), Color32::BLACK);
}

fn length_label(unit: LengthUnit, meters: f64) -> String {
    format!("{:.2} {}", unit.from_meters(meters), unit.label())
}

/// The tool's transient feedback, over the plan.
pub(super) fn preview(painter: &Painter, f: &Frame, preview: &Preview, data: &PlanningData, unit: LengthUnit) {
    match preview {
        Preview::None => {}
        Preview::Cursor { at, kind } => {
            let color = match kind {
                SnapKind::Endpoint => OK,
                SnapKind::Angle => Color32::from_rgb(235, 140, 20),
                SnapKind::Grid | SnapKind::None => Color32::GRAY,
            };
            painter.circle_stroke(f.to_screen(*at), 5.0, Stroke::new(1.5, color));
        }
        Preview::Segment { a, b, thickness_m } => {
            let (pa, pb) = (f.to_screen(*a), f.to_screen(*b));
            painter.line_segment([pa, pb], Stroke::new(f.length_px(*thickness_m).max(MIN_WALL_PX), AP.gamma_multiply(0.6)));
            painter.circle_stroke(pb, 5.0, Stroke::new(1.5, AP));
            let meters = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
            if meters > 0.0 {
                painter.text(pos2((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0) + vec2(0.0, -10.0), Align2::CENTER_BOTTOM, length_label(unit, meters), FontId::proportional(13.0), Color32::BLACK);
            }
        }
        Preview::Polygon { points, cursor, can_close } => {
            let mut pts: Vec<_> = points.iter().map(|p| f.to_screen(*p)).collect();
            if let Some(c) = cursor {
                pts.push(f.to_screen(*c));
            }
            painter.add(Shape::line(pts.clone(), Stroke::new(2.0, CUTOUT)));
            for p in points {
                painter.circle_filled(f.to_screen(*p), VERTEX_RADIUS_PX, CUTOUT);
            }
            if *can_close
                && let Some(first) = points.first()
            {
                painter.circle_stroke(f.to_screen(*first), 8.0, Stroke::new(2.0, OK));
            }
        }
        Preview::Opening { wall, start, end, ok } => {
            if let Some(w) = data.walls.iter().find(|w| w.id == *wall) {
                let (a, b) = (f.to_screen(along(w, *start)), f.to_screen(along(w, *end)));
                painter.line_segment([a, b], Stroke::new(wall_width(f, w) + 3.0, if *ok { OK } else { BAD }.gamma_multiply(0.7)));
            }
        }
        Preview::Ap(p) => {
            painter.circle(f.to_screen(*p), AP_RADIUS_PX, AP.gamma_multiply(0.5), Stroke::new(1.5, Color32::WHITE));
        }
    }
}
