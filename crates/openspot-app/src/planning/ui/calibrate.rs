//! "Calibrate to survey" window. UNVERIFIED: egui layer written without a display; the fit
//! and its inputs are unit-tested in `calibrate_fit`.

use eframe::egui;

use crate::planning::Planning;
use crate::survey::Survey;

pub(super) fn window(p: &mut Planning, s: &Survey, ctx: &egui::Context) {
    if !p.calibration.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Calibrate to survey").open(&mut open).default_width(430.0).show(ctx, |ui| {
        ui.label("Adjusts the path-loss exponent and offset so the prediction matches a Survey snapshot of this zone. Place your real access points first, with their real power, channel and position, then pick which networks are yours.");
        snapshot_picker(p, s, ui);
        ui.separator();
        outcome(p, s, ui);
    });
    if !open {
        p.calibration.open = false;
    }
}

fn snapshot_picker(p: &mut Planning, s: &Survey, ui: &mut egui::Ui) {
    let snapshots = &s.zone().snapshots;
    if snapshots.is_empty() {
        ui.weak("This zone has no survey data. Measure in Survey mode first.");
        return;
    }
    p.calibration.snapshot = p.calibration.snapshot.min(snapshots.len() - 1);
    let current = &snapshots[p.calibration.snapshot];
    let before = p.calibration.snapshot;
    egui::ComboBox::from_id_salt("calibration_snapshot").width(260.0).selected_text(format!("{} ({} points)", current.name, current.points.len())).show_ui(ui, |ui| {
        for (i, snap) in snapshots.iter().enumerate() {
            ui.selectable_value(&mut p.calibration.snapshot, i, format!("{} ({} points)", snap.name, snap.points.len()));
        }
    });
    if p.calibration.snapshot != before {
        p.calibration.networks.clear();
        p.calibration.outcome = None;
    }

    ui.strong("Which networks are the planned ones?");
    ui.weak("Each point's best signal among the ticked networks is compared with the planned radio of the same band. None ticked uses every network heard.");
    let networks = snapshots[p.calibration.snapshot].networks();
    ui.horizontal(|ui| {
        if ui.small_button("All").clicked() {
            p.calibration.networks = networks.iter().map(|n| n.0).collect();
        }
        if ui.small_button("None").clicked() {
            p.calibration.networks.clear();
        }
    });
    egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
        for (bssid, ssid, best) in networks {
            let mut on = p.calibration.networks.contains(&bssid);
            let name = if ssid.is_empty() { "(hidden)".to_owned() } else { ssid };
            if ui.checkbox(&mut on, format!("{name}  {bssid}  best {best} dBm")).changed() {
                if on {
                    p.calibration.networks.insert(bssid);
                } else {
                    p.calibration.networks.remove(&bssid);
                }
            }
        }
    });
}

fn outcome(p: &mut Planning, s: &Survey, ui: &mut egui::Ui) {
    let has_points = s.zone().snapshots.get(p.calibration.snapshot).is_some_and(|snap| !snap.points.is_empty());
    ui.horizontal(|ui| {
        if ui.add_enabled(has_points, egui::Button::new("Fit")).on_hover_text("Needs at least 3 comparable survey points spread over different distances").clicked() {
            p.run_calibration(&s.project, s.zone);
        }
        if ui.button("Reset to default").on_hover_text("Go back to the uncalibrated model").clicked() {
            p.reset_calibration();
        }
    });
    match &p.calibration.outcome {
        None => {
            let m = p.predict_settings.model;
            ui.weak(format!("In use: exponent {:.2}, offset {:+.1} dB.", m.exponent, m.offset_db));
        }
        Some(Err(e)) => {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        Some(Ok(r)) => {
            egui::Grid::new("calibration_report").num_columns(3).striped(true).show(ui, |ui| {
                ui.label("");
                ui.strong("Before");
                ui.strong("Fitted");
                ui.end_row();
                ui.label("Path-loss exponent");
                ui.label(format!("{:.2}", r.before_model.exponent));
                ui.label(format!("{:.2}", r.fit.exponent));
                ui.end_row();
                ui.label("Offset");
                ui.label(format!("{:+.1} dB", r.before_model.offset_db));
                ui.label(format!("{:+.1} dB", r.fit.offset_db));
                ui.end_row();
                ui.label("Error (RMSE)");
                ui.label(format!("{:.1} dB", r.before_rmse_db));
                ui.label(format!("{:.1} dB", r.after_rmse_db));
                ui.end_row();
            });
            ui.label(format!("{} survey points compared ({}).", r.fit.points, r.skipped.describe()));
            if r.fit.points < 6 {
                ui.colored_label(egui::Color32::YELLOW, "Few points: the error figure is optimistic with so little data. Survey more spots, including behind walls.");
            }
            if ui.button("Apply to the prediction").clicked() {
                p.apply_calibration();
            }
        }
    }
}
