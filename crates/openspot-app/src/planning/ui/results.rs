//! Prediction controls: run/stop, detail, view modes, styling, legend and PNG export.
//! UNVERIFIED: egui layer written without a display; the logic behind it is unit-tested.

use eframe::egui;
use openspot_model::Band;
use openspot_predict::Mode;

use crate::heat_ui;
use crate::planning::predict_input::{DETAIL_CELLS, Prediction};
use crate::planning::results::{Legend, ViewMode, cycle_radio};
use crate::planning::Planning;
use crate::survey::Survey;

/// Why signal-to-noise maps are not offered; shown as the tooltip of the disabled option.
const NO_SNR_REASON: &str = "SNR needs a noise-floor value and OpenSpot has no noise source on any platform yet (see docs/NOISE_FLOOR.md), so SNR maps are not offered. Predicted values are signal levels; the interference view shows co-channel contention between your planned access points.";

pub(super) fn panel(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    ui.heading("Prediction");
    controls(p, ui);
    status(p, s, ui);
    let Some(pred) = p.shown().and_then(|sh| sh.prediction.clone()) else { return };
    ui.separator();
    view_modes(p, &pred, ui);
    ui.separator();
    style(p, ui);
    legend(p, ui);
    ui.horizontal(|ui| {
        if ui.button("Export PNG…").clicked() {
            export(p, s);
        }
    });
}

fn controls(p: &mut Planning, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut p.auto_predict, "Update automatically");
        let running = p.prediction_progress().is_some();
        if ui.add_enabled(!running, egui::Button::new("Predict now")).clicked() {
            p.predict_requested = true;
        }
        if running && ui.button("Stop").clicked() {
            p.cancel_prediction();
        }
    });
    ui.horizontal(|ui| {
        ui.label("Detail");
        egui::ComboBox::from_id_salt("predict_detail").selected_text(DETAIL_CELLS[p.predict_settings.detail].0).show_ui(ui, |ui| {
            for (i, (label, _)) in DETAIL_CELLS.iter().enumerate() {
                ui.selectable_value(&mut p.predict_settings.detail, i, *label);
            }
        });
        ui.label("Client height");
        ui.add(egui::DragValue::new(&mut p.predict_settings.rx_height_m).range(0.1..=3.0).speed(0.05).suffix(" m"));
    });
    let mut bounce = p.predict_settings.mode == Mode::SingleBounce;
    if ui.checkbox(&mut bounce, "Wall reflections (slower)").on_hover_text("Adds single-bounce reflections off walls. Not covered by calibration.").changed() {
        p.predict_settings.mode = if bounce { Mode::SingleBounce } else { Mode::Direct };
    }
    let m = p.predict_settings.model;
    let calibrated = m != openspot_predict::PropagationModel::default();
    ui.weak(format!("Path loss: exponent {:.2}, offset {:+.1} dB ({})", m.exponent, m.offset_db, if calibrated { "calibrated" } else { "default, not calibrated" }));
    if ui.button("Suggest AP positions…").on_hover_text("Find spots for new access points that improve coverage").clicked() {
        p.suggest.open = true;
    }
    if ui.button("Calibrate to survey…").on_hover_text("Fit the path-loss model to measurements of this zone").clicked() {
        p.calibration.open = true;
    }
}

fn status(p: &Planning, s: &Survey, ui: &mut egui::Ui) {
    if let Some(e) = &p.predict_error {
        ui.colored_label(egui::Color32::LIGHT_RED, e);
        return;
    }
    if p.prediction_progress().is_some() {
        ui.label("Computing…");
        return;
    }
    if p.prediction_stopped(&s.project, s.zone) {
        ui.colored_label(egui::Color32::YELLOW, "Stopped. Press Predict now to compute this plan.");
        return;
    }
    match p.shown() {
        None => {
            ui.weak("Place an access point and set the scale to see a prediction.");
        }
        Some(sh) if sh.prediction.is_none() => {
            ui.weak("No enabled radio on any floor: place an access point or switch a radio on.");
        }
        Some(sh) => {
            if !p.shown_is_current(&s.project, s.zone) {
                ui.colored_label(egui::Color32::YELLOW, "Out of date: the plan changed. Press Predict now.");
            }
            for w in sh.prediction.iter().flat_map(|pr| &pr.warnings) {
                ui.colored_label(egui::Color32::YELLOW, w);
            }
        }
    }
}

fn view_modes(p: &mut Planning, pred: &Prediction, ui: &mut egui::Ui) {
    ui.strong("Show");
    let v = &mut p.view_settings;
    ui.radio_value(&mut v.mode, ViewMode::Strongest, "Strongest signal");
    ui.radio_value(&mut v.mode, ViewMode::ApCount, "Number of access points");
    ui.radio_value(&mut v.mode, ViewMode::Sir, "Interference (signal to interference)");
    ui.radio_value(&mut v.mode, ViewMode::Serving, "Serving access point");
    ui.add_enabled(false, egui::RadioButton::new(false, "Signal to noise")).on_disabled_hover_text(NO_SNR_REASON);
    ui.horizontal(|ui| {
        let by_ap = matches!(v.mode, ViewMode::Radio { .. });
        if ui.radio(by_ap, "One radio").clicked() && !by_ap {
            if let Some(m) = cycle_radio(pred, v.mode, 0) {
                v.mode = m;
            }
        }
        if by_ap {
            if ui.small_button("◀").clicked() {
                v.mode = cycle_radio(pred, v.mode, -1).unwrap_or(v.mode);
            }
            if ui.small_button("▶").clicked() {
                v.mode = cycle_radio(pred, v.mode, 1).unwrap_or(v.mode);
            }
        }
    });
    if let ViewMode::Radio { ap, radio_index } = v.mode {
        let current = pred.radios.iter().find(|r| r.ap_id == ap && r.radio_index == radio_index).map_or_else(String::new, |r| r.label());
        egui::ComboBox::from_id_salt("view_radio").width(220.0).selected_text(current).show_ui(ui, |ui| {
            for r in &pred.radios {
                if ui.selectable_label(r.ap_id == ap && r.radio_index == radio_index, r.label()).clicked() {
                    v.mode = ViewMode::Radio { ap: r.ap_id, radio_index: r.radio_index };
                }
            }
        });
    }
    let by_radio = matches!(v.mode, ViewMode::Radio { .. });
    ui.add_enabled_ui(!by_radio, |ui| {
        ui.horizontal(|ui| {
            ui.label("Band");
            ui.selectable_value(&mut v.band, None, "All");
            for band in Band::ALL.into_iter().filter(|b| pred.radios.iter().any(|r| r.band == *b)) {
                ui.selectable_value(&mut v.band, Some(band), band.label());
            }
        });
    });
}

fn style(p: &mut Planning, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        heat_ui::gradient_combo(ui, "planning_gradient", &mut p.view_settings.gradient);
        heat_ui::smoothing_combo(ui, "planning_smoothing", &mut p.view_settings.smoothing);
    });
    heat_ui::opacity_slider(ui, &mut p.view_settings.opacity);
    ui.checkbox(&mut p.show_prediction, "Show on plan");
}

fn legend(p: &mut Planning, ui: &mut egui::Ui) {
    let Some(image) = p.heat_image() else {
        ui.weak("Nothing to show for this selection.");
        return;
    };
    ui.strong("Legend");
    match &image.legend {
        Legend::Gradient { gradient, unit } => heat_ui::gradient_bar(ui, gradient, unit),
        Legend::Radios(entries) => {
            for ([r, g, b], label) in entries {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(*r, *g, *b), "■");
                    ui.label(label);
                });
            }
        }
    }
}

fn export(p: &mut Planning, s: &mut Survey) {
    let default = format!("{} - prediction.png", s.zone().name);
    let Some(path) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name(default).save_file() else { return };
    let zone = s.zone;
    let result = p.export_png(&s.project, zone).and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display())));
    p.notice = Some(match result {
        Ok(()) => format!("exported {}", path.display()),
        Err(e) => e,
    });
}
