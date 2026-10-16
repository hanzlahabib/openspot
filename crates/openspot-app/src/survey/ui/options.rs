//! Survey UI: sampling and canvas options menu.

use eframe::egui;
use openspot_model::{MAX_SAMPLES_PER_POINT, RadiusPreset};

use crate::survey::{ApMarkerSize, SampleDetail, Survey};

pub(super) fn sampling(s: &mut Survey, ui: &mut egui::Ui) {
    ui.label("Data point radius");
    let current = s.radius_preset();
    if ui.selectable_label(current.is_none(), "Automatic").clicked() {
        s.set_radius_preset(None);
    }
    for preset in RadiusPreset::ALL {
        let text = format!("{} · {} m", preset.label(), preset.meters());
        if ui.selectable_label(current == Some(preset), text).clicked() {
            s.set_radius_preset(Some(preset));
        }
    }
    if current.is_some() && s.zone().scale.is_none() {
        ui.colored_label(egui::Color32::YELLOW, "Set the scale first; presets are real distances.");
    }
    ui.separator();
    let mut n = s.samples_per_point();
    ui.horizontal(|ui| {
        ui.label("Measurements per point");
        ui.add(egui::DragValue::new(&mut n).range(1..=MAX_SAMPLES_PER_POINT));
    })
    .response
    .on_hover_text("Scans averaged into each point (power average). More scans smooth out fading but take longer.");
    s.set_samples_per_point(n);
}

pub(super) fn hidden_networks(s: &mut Survey, ui: &mut egui::Ui) {
    let rows = s.hidden_rows();
    if rows.is_empty() {
        ui.label("No hidden networks in this snapshot.");
        return;
    }
    ui.label("Hidden networks (no SSID) in this snapshot");
    for row in rows {
        let bssid = row.sighting.bssid;
        ui.horizontal(|ui| {
            let mut included = row.included;
            if ui.checkbox(&mut included, "").on_hover_text("Include in heatmap and exports").changed() {
                s.set_hidden_included(bssid, included);
            }
            ui.monospace(bssid.to_string());
            ui.label(format!("{} dBm · {} pts", row.sighting.best_rssi_dbm, row.sighting.points));
            let mut label = row.label.clone();
            if ui.add(egui::TextEdit::singleline(&mut label).hint_text("name").desired_width(110.0)).changed() {
                s.set_hidden_label(bssid, &label);
            }
        });
    }
}

pub(super) fn ap_positions(s: &mut Survey, ui: &mut egui::Ui) {
    ui.label("Select a network in the list, then drag its marker on the map to place it by hand.");
    let markers = s.ap_markers();
    if markers.is_empty() {
        ui.label("No markers: select networks with at least three readings, or place one by hand.");
    }
    for m in markers {
        ui.horizontal(|ui| {
            ui.label(format!("{} {}", if m.ssid.is_empty() { m.bssid.to_string() } else { m.ssid.clone() }, if m.manual { "(manual)" } else { "(estimated)" }));
            if m.manual && ui.small_button("Reset").on_hover_text("Go back to the automatic estimate").clicked() {
                s.reset_manual_ap(&m.bssid);
            }
        });
    }
    ui.separator();
    if ui.add_enabled(s.snapshot_has_manual_aps(), egui::Button::new("Reset all manual positions")).clicked() {
        s.reset_all_manual_aps();
    }
}

pub(super) fn visibility(s: &mut Survey, ui: &mut egui::Ui) {
    ui.checkbox(&mut s.show_floor_plan, "Floor plan");
    ui.checkbox(&mut s.show_path, "Survey path");
    ui.separator();
    ui.checkbox(&mut s.show_points, "Sample indicators");
    ui.add_enabled_ui(s.show_points, |ui| {
        for detail in SampleDetail::ALL {
            ui.radio_value(&mut s.sample_detail, detail, detail.label());
        }
    });
    ui.separator();
    ui.checkbox(&mut s.show_ap_markers, "AP markers");
    ui.add_enabled_ui(s.show_ap_markers, |ui| {
        ui.checkbox(&mut s.show_ap_titles, "AP titles");
        for size in ApMarkerSize::ALL {
            ui.radio_value(&mut s.ap_marker_size, size, size.label());
        }
    });
}
