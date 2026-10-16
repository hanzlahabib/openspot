//! "Suggest AP positions" window and the markers it puts on the plan.
//! UNVERIFIED: egui layer written without a display; the computation is unit-tested.

use eframe::egui;
use openspot_model::Band;

use crate::planning::Planning;
use crate::planning::suggest::{MAX_COUNT, SuggestParams};
use crate::survey::Survey;

pub(super) fn window(p: &mut Planning, s: &mut Survey, ctx: &egui::Context) {
    p.poll_suggestions();
    if p.suggestions_progress().is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
    if !p.suggest.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Suggest access point positions").open(&mut open).default_width(400.0).show(ctx, |ui| {
        ui.label("Finds spots for new access points that bring the most of the plan up to the signal level you choose, one after another, on top of the access points already placed.");
        ui.weak("Coverage only: this does not plan channels or weigh interference between access points.");
        params(p, ui);
        run_controls(p, s, ui);
        ui.separator();
        results(p, s, ui);
    });
    if !open {
        p.suggest.open = false;
    }
}

fn params(p: &mut Planning, ui: &mut egui::Ui) {
    let SuggestParams { count, threshold_dbm, band, spacing_m } = &mut p.suggest.params;
    egui::Grid::new("suggest_params").num_columns(2).show(ui, |ui| {
        ui.label("Number of new access points");
        ui.add(egui::DragValue::new(count).range(1..=MAX_COUNT));
        ui.end_row();
        ui.label("Cover to at least");
        ui.add(egui::DragValue::new(threshold_dbm).range(-90.0..=-30.0).suffix(" dBm")).on_hover_text("-67 dBm is a common target for voice and video");
        ui.end_row();
        ui.label("Band");
        egui::ComboBox::from_id_salt("suggest_band").selected_text(band.label()).show_ui(ui, |ui| {
            for b in Band::ALL {
                ui.selectable_value(band, b, b.label());
            }
        });
        ui.end_row();
        ui.label("Candidate spacing");
        ui.add(egui::DragValue::new(spacing_m).range(0.5..=20.0).speed(0.1).suffix(" m")).on_hover_text("Distance between the positions tried. Larger plans use wider spacing automatically.");
        ui.end_row();
    });
    ui.weak(format!("New access points use the model chosen under \"New access point\": {}.", p.settings.ap_model.name));
}

fn run_controls(p: &mut Planning, s: &Survey, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        match p.suggestions_progress() {
            Some(progress) => {
                ui.add(egui::ProgressBar::new(progress).desired_width(150.0).text("searching"));
                if ui.button("Stop").clicked() {
                    p.cancel_suggestions();
                }
            }
            None => {
                if ui.button("Suggest").clicked() {
                    p.start_suggestions(&s.project, s.zone);
                }
            }
        }
    });
}

fn results(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    let zone = s.zone;
    match &p.suggest.results {
        None => {
            ui.weak("No suggestions yet.");
        }
        Some(Err(e)) => {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        Some(Ok(list)) if list.is_empty() => {
            ui.label("Nothing to add: no position brings additional coverage at this level.");
        }
        Some(Ok(_)) => {
            if !p.suggestions_are_current() {
                ui.colored_label(egui::Color32::YELLOW, "The plan changed since these were computed. Suggest again to refresh.");
            }
            let list = p.suggestions_for(s.zone().id).to_vec();
            let mut accept = None;
            for (i, sug) in list.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}. at ({:.1}, {:.1}) m: +{:.0} % of the plan, {:.0} % covered in total", i + 1, sug.position.x, sug.position.y, 100.0 * sug.gain_fraction, 100.0 * sug.covered_fraction));
                    if ui.small_button("Add").clicked() {
                        accept = Some(i);
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.button("Add all").clicked() {
                    p.accept_all_suggestions(&mut s.project, zone);
                }
                if ui.button("Clear").clicked() {
                    p.clear_suggestions();
                }
            });
            if let Some(i) = accept {
                p.accept_suggestion(&mut s.project, zone, i);
            }
        }
    }
}
