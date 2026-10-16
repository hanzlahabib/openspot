//! egui layer for Survey mode.

use eframe::egui::{self, Color32};
use crate::survey::{Survey};
use crate::aliases::Aliases;
use crate::groups::CustomGroups;
use crate::worker::{Worker};

mod active_scan;
mod ap;
mod calib_rect;
mod canvas;
mod contours;
mod crop;
mod dialogs;
mod draw;
mod explorer;
mod merge;
mod options;
mod path;
mod pdf;
mod per_ap;
mod range;
mod report;
mod report_job;
mod side;
mod start;
mod recovery;
mod toolbar;

pub use report::ReportUi;
pub(crate) use toolbar::save as save_project;

pub fn show(s: &mut Survey, ctx: &egui::Context, worker: &Worker, aliases: &mut Aliases, groups: &CustomGroups) {
    if s.explorer.start_page {
        start::page(s, ctx);
        return;
    }
    handle_shortcuts(s, ctx);
    s.tick_autosave(std::time::Instant::now());
    if s.dirty && s.autosave.interval_secs > 0 {
        // egui only repaints on input; wake up in time for the next autosave check.
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
    }
    draw::keys(s, ctx);
    path::drive(s, ctx, worker);
    egui::TopBottomPanel::top("survey_bar").show(ctx, |ui| toolbar::toolbar(s, ui));
    if s.tool == crate::survey::Tool::Path {
        egui::TopBottomPanel::top("survey_path_bar").show(ctx, |ui| path::bar(s, ui, ctx));
    }
    if s.tool == crate::survey::Tool::Draw {
        egui::TopBottomPanel::top("survey_draw_bar").show(ctx, |ui| draw::bar(s, ui));
    }
    if s.tool == crate::survey::Tool::Crop {
        egui::TopBottomPanel::top("survey_crop_bar").show(ctx, |ui| crop::bar(s, ui));
    }
    egui::TopBottomPanel::bottom("survey_status").show(ctx, |ui| status(s, ui));
    if s.explorer.visible {
        egui::SidePanel::left("survey_explorer").default_width(200.0).show(ctx, |ui| explorer::panel(s, ui));
    }
    egui::SidePanel::right("survey_networks").default_width(260.0).show(ctx, |ui| side::networks(s, ui, aliases, groups));
    egui::CentralPanel::default().show(ctx, |ui| canvas::canvas(s, ui, aliases));
    dialogs::calibration_dialog(s, ctx);
    calib_rect::dialog(s, ctx);
    dialogs::blank_dialog(s, ctx);
    dialogs::manage_dialog(s, ctx);
    active_scan::settings_window(s, ctx);
    report::show(s, ctx, aliases);
    recovery::dialog(s, ctx);
    merge::dialog(s, ctx);
    draw::dialogs(s, ctx);
    pdf::dialog(s, ctx);
    crate::ui::alias::dialog(ctx, aliases, &mut s.alias_edit);
}

/// Ctrl/Cmd+Z undo, Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z redo. Skipped while typing in a text field.
fn handle_shortcuts(s: &mut Survey, ctx: &egui::Context) {
    if ctx.wants_keyboard_input() {
        return;
    }
    use egui::{Key, Modifiers};
    let (redo, undo) = ctx.input_mut(|i| {
        let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y);
        (redo, !redo && i.consume_key(Modifiers::COMMAND, Key::Z))
    });
    if redo {
        s.redo();
    } else if undo {
        s.undo();
    }
}

fn status(s: &Survey, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(format!(
            "{}{} · {} points",
            s.project.name,
            if s.dirty { " •" } else { "" },
            s.snapshot().points.len()
        ));
        match s.zone().scale {
            Some(sc) => {
                let size = s.zone().map.as_ref().map_or(String::new(), |m| {
                    let (w, h) = (sc.to_meters(f64::from(m.width)), sc.to_meters(f64::from(m.height)));
                    format!(" · plan {:.1} × {:.1} {}", s.unit.from_meters(w), s.unit.from_meters(h), s.unit.label())
                });
                ui.label(format!("· {:.1} px/m{size}", sc.pixels_per_meter()))
            }
            None if s.zone().map.is_some() => ui.colored_label(Color32::YELLOW, "· scale not set"),
            None => ui.label(""),
        };
        if s.busy {
            ui.spinner();
            ui.label("scanning…");
            active_scan::status(s, ui);
        }
        if let Some(p) = s.heat_progress() {
            ui.add(egui::ProgressBar::new(p).desired_width(90.0).text("heatmap"));
        }
        if let Some(e) = &s.error {
            ui.colored_label(Color32::LIGHT_RED, format!("· {e}"));
        } else if let Some(m) = &s.message {
            ui.label(format!("· {m}"));
        }
    });
}
