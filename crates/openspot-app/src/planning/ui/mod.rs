//! egui layer for Planning mode. UNVERIFIED: written without a display; the logic it calls
//! (tools, edits, materials, models) is unit-tested in the parent modules.

use eframe::egui::{self, Color32, Key, Modifiers};

use super::Planning;
use crate::survey::Survey;

mod ap;
mod calibrate;
mod canvas;
mod draw;
mod frame;
mod level;
mod materials;
mod results;
mod setup;
mod side;
mod suggest;
mod toolbar;

pub(super) use canvas::MapTexture;

pub fn show(p: &mut Planning, s: &mut Survey, ctx: &egui::Context) {
    let zone_id = s.zone().id;
    p.sync_zone(s.project.id, zone_id);
    handle_keys(p, s, ctx);
    let force = std::mem::take(&mut p.predict_requested);
    p.advance_prediction(&s.project, s.zone, force);
    if p.prediction_progress().is_some() {
        // egui only repaints on input; poll the worker while it runs.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
    egui::TopBottomPanel::top("planning_bar").show(ctx, |ui| toolbar::bar(p, s, ui));
    if s.zone().map.is_some() && s.zone().scale.is_none() {
        egui::TopBottomPanel::top("planning_scale").show(ctx, |ui| setup::scale_banner(p, s, ui));
    }
    egui::TopBottomPanel::bottom("planning_status").show(ctx, |ui| status(p, s, ui));
    egui::SidePanel::right("planning_side").default_width(300.0).show(ctx, |ui| side::panel(p, s, ui));
    egui::CentralPanel::default().show(ctx, |ui| canvas::canvas(p, s, ui));
    materials::window(p, s, ctx);
    calibrate::window(p, s, ctx);
    suggest::window(p, s, ctx);
    if p.take_changed() {
        s.dirty = true;
    }
}

/// Ctrl/Cmd+Z, Ctrl/Cmd+Y (or +Shift+Z), Esc, Delete and Enter. Skipped while typing in a field.
fn handle_keys(p: &mut Planning, s: &mut Survey, ctx: &egui::Context) {
    if ctx.wants_keyboard_input() {
        return;
    }
    let (redo, undo, escape, delete, enter) = ctx.input_mut(|i| {
        let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y);
        let undo = !redo && i.consume_key(Modifiers::COMMAND, Key::Z);
        let delete = i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace);
        (redo, undo, i.consume_key(Modifiers::NONE, Key::Escape), delete, i.consume_key(Modifiers::NONE, Key::Enter))
    });
    let zone = s.zone;
    if redo {
        p.redo(&mut s.project);
    } else if undo {
        p.undo(&mut s.project);
    }
    if escape {
        if p.editor.is_drafting() || p.editor.is_grabbing() {
            p.editor.cancel();
        } else {
            p.editor.selection = None;
        }
    }
    // Pick distance is irrelevant for keys: nothing is hit-tested.
    if delete {
        p.delete_selection(&mut s.project, zone, 0.0);
    }
    if enter {
        p.confirm(&mut s.project, zone, 0.0);
    }
}

fn status(p: &Planning, s: &Survey, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let data = &s.zone().planning;
        ui.label(format!("{}{} · {} walls · {} access points", s.project.name, if s.dirty { " •" } else { "" }, data.walls.len(), data.access_points.len()));
        match s.zone().scale {
            Some(sc) => ui.label(format!("· {:.1} px/m", sc.pixels_per_meter())),
            None if s.zone().map.is_some() => ui.colored_label(Color32::YELLOW, "· scale not set"),
            None => ui.label(""),
        };
        if let Some(progress) = p.prediction_progress() {
            ui.add(egui::ProgressBar::new(progress).desired_width(110.0).text("predicting"));
        }
        if let Some(e) = &s.error {
            ui.colored_label(Color32::LIGHT_RED, format!("· {e}"));
        } else if let Some(n) = p.notice.as_ref().or(s.message.as_ref()) {
            ui.label(format!("· {n}"));
        }
    });
}
