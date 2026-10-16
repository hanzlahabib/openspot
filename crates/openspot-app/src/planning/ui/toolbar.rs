//! Planning toolbar: tools, undo/redo, snapping and drawing defaults.

use eframe::egui;
use openspot_model::MaterialCategory;

use super::materials::picker;
use crate::planning::Planning;
use crate::planning::geometry::GRID_PRESETS_M;
use crate::planning::tools::{THICKNESS_PRESETS_M, Tool};
use crate::survey::Survey;

pub(super) fn bar(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for tool in Tool::ALL {
            if ui.selectable_label(p.editor.tool() == tool, tool.label()).on_hover_text(tool.hint()).clicked() {
                p.set_tool(tool);
            }
        }
        ui.separator();
        super::level::selector(s, ui);
        ui.separator();
        if ui.add_enabled(p.can_undo(), egui::Button::new("Undo")).clicked() {
            p.undo(&mut s.project);
        }
        if ui.add_enabled(p.can_redo(), egui::Button::new("Redo")).clicked() {
            p.redo(&mut s.project);
        }
        ui.separator();
        if ui.button("Fit").clicked() {
            p.zoom_fit_requested = true;
        }
        if ui.button("Save").on_hover_text("Saves the whole project, Survey data included").clicked() {
            crate::survey::save_project(s, false);
        }
    });
    ui.horizontal_wrapped(|ui| {
        snapping(p, ui);
        ui.separator();
        ui.label("New wall:");
        for (label, m) in THICKNESS_PRESETS_M {
            ui.selectable_value(&mut p.settings.thickness_m, m, label);
        }
        let data = &s.zone().planning;
        if let Some(id) = picker(ui, "wall_material", data, &p.catalog, MaterialCategory::Wall, &p.settings.wall_material) {
            p.settings.wall_material = id;
        }
        ui.separator();
        if ui.button("Materials…").on_hover_text("Create and edit custom materials").clicked() {
            p.open_material_editor();
        }
    });
}

fn snapping(p: &mut Planning, ui: &mut egui::Ui) {
    let snap = &mut p.settings.snap;
    ui.checkbox(&mut snap.grid, "Grid");
    egui::ComboBox::from_id_salt("planning_grid").width(70.0).selected_text(format!("{} m", snap.grid_m)).show_ui(ui, |ui| {
        for g in GRID_PRESETS_M {
            ui.selectable_value(&mut snap.grid_m, g, format!("{g} m"));
        }
    });
    ui.checkbox(&mut snap.angle, "Angles").on_hover_text("Lock the direction of a new wall to 15 degree steps");
    ui.checkbox(&mut snap.endpoints, "Endpoints").on_hover_text("Snap to the ends of existing walls and stairwell corners");
}
