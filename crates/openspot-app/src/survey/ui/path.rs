//! Survey UI: path mode controls and route drawing.

use eframe::egui::{self, Color32, Pos2, Stroke};

use crate::survey::Survey;
use crate::survey::path::RunState;
use crate::worker::Worker;

const ROUTE_COLOR: Color32 = Color32::from_rgb(30, 120, 255);
const WAYPOINT_RADIUS: f32 = 4.0;
const ROUTE_WIDTH: f32 = 2.5;
/// While a run is active the schedule needs frames even when the user is idle.
const RUN_REPAINT_MS: u64 = 200;

/// Forwards scan requests to the worker and advances an active run.
pub(super) fn drive(s: &mut Survey, ctx: &egui::Context, worker: &Worker) {
    s.drive_path(ctx.input(|i| i.time));
    if let Some((x, y)) = s.take_pending_scan() {
        s.dispatch_scan(worker, x, y);
    }
    if s.path_running() {
        ctx.request_repaint_after(std::time::Duration::from_millis(RUN_REPAINT_MS));
    }
}

pub(super) fn bar(s: &mut Survey, ui: &mut egui::Ui, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    ui.horizontal_wrapped(|ui| {
        let running = s.path_running();
        let unit = s.unit;
        ui.add_enabled_ui(!running, |ui| {
            ui.label("Sample every");
            let mut shown = unit.from_meters(s.path_interval_m());
            let range = unit.from_meters(0.2)..=unit.from_meters(100.0);
            if ui.add(egui::DragValue::new(&mut shown).speed(0.1).range(range).suffix(format!(" {}", unit.label()))).changed() {
                s.set_path_interval_m(unit.to_meters(shown));
            }
            ui.label("walking at");
            ui.add(egui::DragValue::new(&mut s.walk_speed_mps).speed(0.05).range(0.1..=3.0).suffix(" m/s"));
            if ui.add_enabled(s.path_waypoints().len() > 1, egui::Button::new("Start")).clicked() {
                if let Err(e) = s.path_start(now) {
                    s.error = Some(e.to_string());
                }
            }
            if ui.add_enabled(!s.path_waypoints().is_empty(), egui::Button::new("Remove last")).clicked() {
                s.path_remove_last();
            }
            if ui.add_enabled(!s.path_waypoints().is_empty(), egui::Button::new("Clear route")).clicked() {
                s.path_clear();
            }
        });
        let Some(active) = &s.path_run else { return };
        let (done, total) = active.run.progress();
        ui.add(egui::ProgressBar::new(done as f32 / total.max(1) as f32).desired_width(160.0).text(format!("{done} / {total}")));
        match active.run.state() {
            RunState::Paused => {
                if ui.button("Resume").clicked() {
                    s.path_resume(now);
                }
            }
            _ => {
                ui.label(format!("next in {:.0} s", active.run.seconds_to_next()));
                if ui.button("Pause").clicked() {
                    s.path_pause(now);
                }
            }
        }
        if ui.button("Cancel").clicked() {
            s.path_cancel();
        }
    });
}

/// Draws the stored route and its waypoints.
pub(super) fn draw(s: &Survey, painter: &egui::Painter, origin: Pos2) {
    if !s.show_path {
        return;
    }
    let points: Vec<Pos2> = s.path_waypoints().iter().map(|&p| s.view.to_screen(origin, p)).collect();
    if points.len() > 1 {
        painter.add(egui::Shape::line(points.clone(), Stroke::new(ROUTE_WIDTH, ROUTE_COLOR)));
    }
    for p in points {
        painter.circle_filled(p, WAYPOINT_RADIUS, ROUTE_COLOR);
    }
}
