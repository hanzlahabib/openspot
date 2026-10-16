//! Survey UI: toolbar.

use eframe::egui::{self};
use openspot_model::LengthUnit;
use crate::survey::{ManageAction, ZoomRequest, MetricChoice, Survey, Tool};

/// Metric menu entry for the transmit-rate map (H-10).
const TX_RATE_LABEL: &str = "Transmit rate (this computer's connection)";

pub(super) fn pick_file(filter_name: &str, exts: &[&str]) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new().add_filter(filter_name, exts).pick_file()
}

pub(super) fn report(s: &mut Survey, result: Result<(), String>, ok: &str) {
    match result {
        Ok(()) => {
            s.error = None;
            s.message = Some(ok.to_owned());
        }
        Err(e) => s.error = Some(e),
    }
}

pub(super) fn toolbar(s: &mut Survey, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("New").clicked() {
            s.new_project();
        }
        if ui.button("Open…").clicked() {
            if let Some(path) = pick_file("OpenSpot project", &["ospot"]) {
                let r = s.open_from(&path);
                report(s, r, "Project opened");
            }
        }
        if ui.button("Save").clicked() {
            save(s, false);
        }
        if ui.button("Save as…").clicked() {
            save(s, true);
        }
        ui.separator();
        if ui.button("Load floor plan…").clicked() {
            if let Some(path) = pick_file("Images and PDF", &["png", "jpg", "jpeg", "pdf"]) {
                let r = s.load_plan(&path);
                report(s, r, "Floor plan loaded. Calibrate the scale next.");
            }
        }
        if ui.button("Blank plan…").on_hover_text("No floor plan? Start from an empty grid of a given size").clicked() {
            s.blank_dialog = Some((20.0, 12.0));
        }
        if ui.button("Draw plan…").on_hover_text("No floor plan? Draw one with lines, rectangles and text").clicked() {
            s.draw_dialog = Some((20.0, 12.0));
        }
        let can_calibrate = s.zone().map.is_some();
        if ui.add_enabled(can_calibrate, egui::Button::new("Calibrate scale")).clicked() {
            s.calibration.start();
        }
        ui.toggle_value(&mut s.explorer.visible, "Explorer");
        if ui.button("Projects").on_hover_text("Start page with recent projects").clicked() {
            s.explorer.start_page = true;
            s.explorer.rows = None;
        }
        if ui.add_enabled(can_calibrate, egui::Button::new("Calibrate by rectangle")).on_hover_text("Click two opposite corners of a room or object of known width and height").clicked() {
            s.calibration.start_rect();
        }
        ui.separator();
        if ui.add_enabled(!s.snapshot().points.is_empty(), egui::Button::new("Export CSV…")).clicked() {
            export(s, ExportKind::Csv);
        }
        if ui.add_enabled(s.zone().map.is_some(), egui::Button::new("Export PNG…")).clicked() {
            export(s, ExportKind::Png);
        }
        super::report::toolbar_buttons(s, ui);
        if ui.add_enabled(s.zone().map.is_some() && s.show_contours, egui::Button::new("Export SVG…")).on_hover_text("Contour lines as scalable vector graphics (turn on Contours)").clicked() {
            export(s, ExportKind::Svg);
        }
        ui.separator();
        ui.selectable_value(&mut s.tool, Tool::Measure, "📍 Measure");
        ui.selectable_value(&mut s.tool, Tool::Path, "🚶 Path");
        ui.selectable_value(&mut s.tool, Tool::Crop, "✂ Crop");
        ui.selectable_value(&mut s.tool, Tool::Draw, "✏ Draw");
        ui.selectable_value(&mut s.tool, Tool::Pan, "✋ Pan");
        if ui.add_enabled(s.can_undo(), egui::Button::new("↶ Undo")).on_hover_text("Ctrl+Z").clicked() {
            s.undo();
        }
        if ui.add_enabled(s.can_redo(), egui::Button::new("↷ Redo")).on_hover_text("Ctrl+Y or Ctrl+Shift+Z").clicked() {
            s.redo();
        }
    });
    ui.horizontal_wrapped(|ui| {
        let zone_names: Vec<String> = s.project.zones.iter().map(|z| z.name.clone()).collect();
        let mut zone = s.zone;
        egui::ComboBox::from_id_salt("zone").selected_text(&zone_names[s.zone]).show_ui(ui, |ui| {
            for (i, name) in zone_names.iter().enumerate() {
                ui.selectable_value(&mut zone, i, name);
            }
        });
        s.select_zone(zone);
        if ui.small_button("+ zone").clicked() {
            s.add_zone();
        }

        let snap_names: Vec<String> = s.zone().snapshots.iter().map(|x| x.name.clone()).collect();
        let mut snap = s.snapshot;
        egui::ComboBox::from_id_salt("snapshot").selected_text(&snap_names[s.snapshot]).show_ui(ui, |ui| {
            for (i, name) in snap_names.iter().enumerate() {
                ui.selectable_value(&mut snap, i, name);
            }
        });
        s.select_snapshot(snap);
        if ui.small_button("+ snapshot").clicked() {
            s.add_snapshot();
        }

        if ui.add_enabled(s.zone().snapshots.len() > 1, egui::Button::new("Merge…")).on_hover_text("Combine snapshots of this zone into a new one").clicked() {
            s.merge_dialog = Some(Default::default());
        }
        ui.menu_button("Manage ▾", |ui| {
            if ui.button("Rename zone…").clicked() {
                s.manage = Some(ManageAction::RenameZone(s.zone().name.clone()));
                ui.close();
            }
            if ui.button("Rename snapshot…").clicked() {
                s.manage = Some(ManageAction::RenameSnapshot(s.snapshot().name.clone()));
                ui.close();
            }
            if ui.button("Duplicate zone").clicked() {
                s.duplicate_zone();
                ui.close();
            }
            if ui.button("Duplicate snapshot").clicked() {
                s.duplicate_snapshot();
                ui.close();
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Autosave every");
                ui.add(egui::DragValue::new(&mut s.autosave.interval_secs).range(0..=crate::survey::MAX_AUTOSAVE_SECS).suffix(" s"))
                    .on_hover_text("0 turns autosave off");
            });
            ui.separator();
            if ui.add_enabled(s.project.zones.len() > 1, egui::Button::new("Delete zone…")).clicked() {
                s.manage = Some(ManageAction::DeleteZone);
                ui.close();
            }
            if ui.add_enabled(s.zone().snapshots.len() > 1, egui::Button::new("Delete snapshot…")).clicked() {
                s.manage = Some(ManageAction::DeleteSnapshot);
                ui.close();
            }
        });
        ui.menu_button("Sampling ▾", |ui| super::options::sampling(s, ui));
        ui.menu_button("Show ▾", |ui| super::options::visibility(s, ui));
        ui.menu_button("AP positions ▾", |ui| super::options::ap_positions(s, ui));
        ui.menu_button("Hidden networks ▾", |ui| super::options::hidden_networks(s, ui));
        ui.separator();
        super::active_scan::toolbar(s, ui);
        ui.separator();
        let choice_before = s.metric_choice;
        egui::ComboBox::from_id_salt("metric")
            .selected_text(match s.metric_choice {
                MetricChoice::BestSignal => "Signal level (best of all)",
                MetricChoice::SelectedSignal => "Signal level (selected networks)",
                MetricChoice::SecondarySignal => "Secondary signal level",
                MetricChoice::Sir => "Signal-to-interference (co-channel)",
                MetricChoice::BandCoverage => "Frequency band coverage",
                MetricChoice::PhyCoverage => "PHY mode coverage",
                MetricChoice::ApCount => "Number of access points",
                MetricChoice::Speed(kind) => kind.label(),
                MetricChoice::TxRate => TX_RATE_LABEL,
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.metric_choice, MetricChoice::BestSignal, "Signal level (best of all)");
                ui.selectable_value(&mut s.metric_choice, MetricChoice::SelectedSignal, "Signal level (selected networks)");
                ui.selectable_value(&mut s.metric_choice, MetricChoice::SecondarySignal, "Secondary signal level");
                ui.selectable_value(&mut s.metric_choice, MetricChoice::Sir, "Signal-to-interference (co-channel)");
                ui.selectable_value(&mut s.metric_choice, MetricChoice::BandCoverage, "Frequency band coverage");
                ui.selectable_value(&mut s.metric_choice, MetricChoice::PhyCoverage, "PHY mode coverage");
                ui.selectable_value(&mut s.metric_choice, MetricChoice::ApCount, "Number of access points");
                ui.selectable_value(&mut s.metric_choice, MetricChoice::TxRate, TX_RATE_LABEL)
                    .on_hover_text("Transmit rate of this computer's own connection; only points measured while connected");
                super::active_scan::metric_entries(s, ui);
            });
        if s.metric_choice != choice_before {
            s.reset_thresholds();
        }
        let can_troubleshoot = s.supports_troubleshooting();
        let was_on = s.troubleshoot;
        ui.add_enabled(can_troubleshoot, egui::Checkbox::new(&mut s.troubleshoot, "Troubleshooting"))
            .on_hover_text("Color regions as Good / Acceptable / Critical against thresholds");
        if s.troubleshoot && !was_on {
            s.reset_thresholds();
        }
        if s.troubleshoot && can_troubleshoot {
            let unit = s.base_metric_unit();
            ui.label("good ≥");
            ui.add(egui::DragValue::new(&mut s.threshold_acceptable).suffix(format!(" {unit}")));
            ui.label("critical ≤");
            ui.add(egui::DragValue::new(&mut s.threshold_critical).suffix(format!(" {unit}")));
        }
        crate::heat_ui::gradient_combo(ui, "gradient", &mut s.gradient);
        crate::heat_ui::smoothing_combo(ui, "smoothing", &mut s.smoothing);
        egui::ComboBox::from_id_salt("unit").selected_text(format!("Units: {}", s.unit.label())).show_ui(ui, |ui| {
            for u in LengthUnit::ALL {
                ui.selectable_value(&mut s.unit, u, u.label());
            }
        });
        egui::ComboBox::from_id_salt("zoom").selected_text(format!("Zoom: {:.0}%", s.view.zoom * 100.0)).show_ui(ui, |ui| {
            if ui.selectable_label(false, "Fit to window").clicked() {
                s.zoom_request = Some(ZoomRequest::Fit);
            }
            for pct in [25, 50, 100, 150, 200] {
                if ui.selectable_label(false, format!("{pct}%")).clicked() {
                    s.zoom_request = Some(ZoomRequest::Percent(pct));
                }
            }
        });
        ui.checkbox(&mut s.show_heatmap, "Heatmap");
        ui.checkbox(&mut s.show_contours, "Contours");
        if s.show_contours {
            ui.add(egui::DragValue::new(&mut s.contour_levels).range(1..=crate::survey::MAX_CONTOUR_LEVELS).suffix(" lines"));
        }
        crate::heat_ui::opacity_slider(ui, &mut s.opacity);
        ui.checkbox(&mut s.show_points, "Points");
        ui.checkbox(&mut s.show_ap_markers, "AP positions");
    });
}

pub(super) enum ExportKind {
    Csv,
    Png,
    Svg,
}

pub(super) fn export(s: &mut Survey, kind: ExportKind) {
    let (name, ext) = match kind {
        ExportKind::Csv => ("CSV", "csv"),
        ExportKind::Png => ("PNG image", "png"),
        ExportKind::Svg => ("SVG contours", "svg"),
    };
    let default = format!("{} - {}.{ext}", s.zone().name, s.snapshot().name);
    let Some(path) = rfd::FileDialog::new().add_filter(name, &[ext]).set_file_name(default).save_file() else { return };
    let bytes = match kind {
        ExportKind::Csv => Ok(openspot_export::snapshot_to_csv(s.zone(), &s.filtered_snapshot()).into_bytes()),
        ExportKind::Svg => s.contours_svg().map(String::into_bytes).map_err(|e| e.to_string()),
        ExportKind::Png => s.render_png(),
    };
    let result = bytes.and_then(|b| std::fs::write(&path, b).map_err(|e| format!("could not write {}: {e}", path.display())));
    report(s, result, "Exported");
}

/// Saves the project (asking for a file when it has none or `force_dialog`), reporting in
/// the survey status line. Planning mode saves through this too.
pub(crate) fn save(s: &mut Survey, force_dialog: bool) {
    let path = match (&s.path, force_dialog) {
        (Some(p), false) => Some(p.clone()),
        _ => rfd::FileDialog::new()
            .add_filter("OpenSpot project", &["ospot"])
            .set_file_name(format!("{}.ospot", s.project.name))
            .save_file(),
    };
    if let Some(path) = path {
        let r = s.save_to(&path);
        report(s, r, "Saved");
    }
}
