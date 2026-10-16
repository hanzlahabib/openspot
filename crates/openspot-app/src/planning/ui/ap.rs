//! Access-point settings panel and the model picker. UNVERIFIED: egui layer written without
//! a display; the value rules live in `ap_edit` and are unit-tested.

use std::ops::RangeInclusive;

use eframe::egui;
use openspot_model::{AntennaPattern, Radio, Uuid, VirtualAp};

use crate::planning::Planning;
use crate::planning::ap_edit::{self, ANTENNA_GAIN_RANGE_DBI, HEIGHT_RANGE_M, MIN_BEAMWIDTH_DEG, PatternKind, TX_POWER_RANGE_DBM};
use crate::planning::channels;
use crate::survey::Survey;

/// A drag value that reports when the user is done with it (released or left the field), so
/// one adjustment is one undo step instead of one per frame.
fn value(ui: &mut egui::Ui, v: &mut f64, range: RangeInclusive<f64>, suffix: &str) -> bool {
    let r = ui.add(egui::DragValue::new(v).range(range).speed(0.1).suffix(suffix));
    r.drag_stopped() || r.lost_focus()
}

/// Which model the AP tool places, and management of the user's own models.
pub(super) fn model_picker(p: &mut Planning, ui: &mut egui::Ui) {
    ui.heading("New access point");
    let current = p.settings.ap_model.name.clone();
    let mut delete = None;
    egui::ComboBox::from_id_salt("ap_model").width(230.0).selected_text(&current).show_ui(ui, |ui| {
        for m in p.models.all() {
            if ui.selectable_label(m.name == current, &m.name).clicked() {
                p.settings.ap_model = m.clone();
            }
        }
    });
    if p.models.user.iter().any(|m| m.name == current)
        && ui.small_button("Delete this saved model").clicked()
    {
        delete = Some(current.clone());
    }
    if let Some(name) = delete {
        p.models.delete_user(&name);
        p.settings.ap_model = p.models.builtin()[0].clone();
    }
    ui.weak("Built-in models are generic classes of access point, not products: set the values of your hardware, then save it as your own model.");
}

pub(super) fn settings(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui, id: Uuid) {
    let zone = s.zone;
    let Some(actual) = s.zone().planning.access_points.iter().find(|a| a.id == id).cloned() else { return };
    let fresh = matches!(&p.ap_draft, Some((synced, draft)) if *synced == actual && draft.id == id);
    if !fresh {
        p.ap_draft = Some((actual.clone(), actual.clone()));
    }
    let mut working = p.ap_draft.as_ref().map_or_else(|| actual.clone(), |(_, d)| d.clone());
    let mut committed = false;
    let mut delete = false;

    ui.horizontal(|ui| {
        ui.label("Name");
        let r = ui.text_edit_singleline(&mut working.name);
        committed |= r.lost_focus();
    });
    ui.horizontal(|ui| {
        ui.label("Mounting height");
        committed |= value(ui, &mut working.height_m, HEIGHT_RANGE_M, " m");
    });
    ui.weak(format!("At ({:.2}, {:.2}) m; drag it on the plan to move.", working.position.x, working.position.y));

    let mut remove = None;
    for (i, radio) in working.radios.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            let header = format!("{} radio", radio.band.label());
            egui::CollapsingHeader::new(header).default_open(true).show(ui, |ui| {
                let (changed, removed) = radio_settings(ui, radio);
                committed |= changed;
                if removed {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        working.radios.remove(i);
        committed = true;
    }
    ui.horizontal(|ui| {
        for band in ap_edit::missing_bands(&working) {
            if ui.small_button(format!("+ {}", band.label())).clicked() {
                working.radios.push(ap_edit::new_radio(band));
                committed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Delete access point").clicked() {
            delete = true;
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Save as model");
        ui.add(egui::TextEdit::singleline(&mut p.model_name).hint_text("name").desired_width(110.0));
        if ui.button("Save").clicked() {
            let name = p.model_name.clone();
            p.save_model(&working, &name);
        }
    });

    if delete {
        p.delete_selection(&mut s.project, zone, 0.0);
        return;
    }
    if committed {
        p.commit_ap(&mut s.project, zone, id, working.clone());
        // Show what was stored (values are clamped on commit).
        p.ap_draft = None;
    } else {
        p.ap_draft = Some((actual, working));
    }
}

/// One radio's controls. Returns (a field was committed, remove requested).
fn radio_settings(ui: &mut egui::Ui, r: &mut Radio) -> (bool, bool) {
    let mut committed = false;
    let mut remove = false;
    ui.horizontal(|ui| {
        committed |= ui.checkbox(&mut r.enabled, "On").changed();
        if ui.small_button("Remove radio").clicked() {
            remove = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Width");
        egui::ComboBox::from_id_salt("width").selected_text(format!("{} MHz", r.width_mhz)).show_ui(ui, |ui| {
            for &w in channels::widths(r.band) {
                if ui.selectable_label(r.width_mhz == w, format!("{w} MHz")).clicked() && ap_edit::set_width(r, w) {
                    committed = true;
                }
            }
        });
        ui.label("Channel");
        egui::ComboBox::from_id_salt("channel").selected_text(r.channel.to_string()).show_ui(ui, |ui| {
            for &c in channels::channels(r.band, r.width_mhz) {
                if ui.selectable_label(r.channel == c, c.to_string()).clicked() {
                    r.channel = c;
                    committed = true;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Power");
        committed |= value(ui, &mut r.tx_power_dbm, TX_POWER_RANGE_DBM, " dBm");
        ui.label("Antenna");
        committed |= value(ui, &mut r.antenna_gain_dbi, ANTENNA_GAIN_RANGE_DBI, " dBi");
    });
    committed |= pattern_settings(ui, r);
    (committed, remove)
}

fn pattern_settings(ui: &mut egui::Ui, r: &mut Radio) -> bool {
    let mut committed = false;
    let kind = PatternKind::of(&r.pattern);
    ui.horizontal(|ui| {
        ui.label("Pattern");
        egui::ComboBox::from_id_salt("pattern").selected_text(kind.label()).show_ui(ui, |ui| {
            for k in [PatternKind::Omni, PatternKind::Directional] {
                if ui.selectable_label(kind == k, k.label()).clicked() {
                    ap_edit::set_pattern_kind(r, k);
                    committed = true;
                }
            }
        });
    });
    match &mut r.pattern {
        AntennaPattern::Omni => {}
        AntennaPattern::Directional { azimuth_deg, h_beamwidth_deg, v_beamwidth_deg, tilt_deg, front_to_back_db } => {
            egui::Grid::new("directional").num_columns(2).show(ui, |ui| {
                ui.label("Direction")
                    .on_hover_text("Bearing of the main beam on the plan: 0 points right, 90 points down the screen.");
                committed |= value(ui, azimuth_deg, -180.0..=180.0, "°");
                ui.end_row();
                ui.label("Beamwidth");
                committed |= value(ui, h_beamwidth_deg, MIN_BEAMWIDTH_DEG..=360.0, "°");
                ui.end_row();
                ui.label("Vertical beamwidth");
                committed |= value(ui, v_beamwidth_deg, MIN_BEAMWIDTH_DEG..=180.0, "°");
                ui.end_row();
                ui.label("Downtilt").on_hover_text("Degrees the beam points below horizontal.");
                committed |= value(ui, tilt_deg, -90.0..=90.0, "°");
                ui.end_row();
                ui.label("Front-to-back");
                committed |= value(ui, front_to_back_db, 0.0..=60.0, " dB");
                ui.end_row();
            });
        }
        AntennaPattern::Table { .. } => {
            ui.weak("Imported antenna table (set through the engine API).");
        }
    }
    committed
}

/// One-line description for the access-point list.
pub(super) fn summary(ap: &VirtualAp) -> String {
    let radios = ap.radios.iter().map(ap_edit::radio_summary).collect::<Vec<_>>().join(" · ");
    format!("{} · {}", ap.name, if radios.is_empty() { "no radios".to_owned() } else { radios })
}
