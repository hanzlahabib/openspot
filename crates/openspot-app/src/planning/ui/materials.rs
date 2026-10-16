//! Material picker and the custom-material editor window.

use eframe::egui;
use openspot_model::{Band, MaterialCatalog, MaterialCategory, PlanningData};

use crate::planning::materials::{self, MaterialDraft};
use crate::planning::{MaterialEditor, Planning};
use crate::survey::Survey;

/// Combo box of the materials of `category`. Returns the id the user picked this frame.
pub(super) fn picker(ui: &mut egui::Ui, id: &str, data: &PlanningData, catalog: &MaterialCatalog, category: MaterialCategory, current: &str) -> Option<String> {
    let mut picked = None;
    egui::ComboBox::from_id_salt(id).width(190.0).selected_text(materials::display_name(data, catalog, current)).show_ui(ui, |ui| {
        for c in materials::choices(data, catalog, category) {
            if ui.selectable_label(c.id == current, &c.label).clicked() {
                picked = Some(c.id);
            }
        }
    });
    picked
}

fn category_label(c: MaterialCategory) -> &'static str {
    match c {
        MaterialCategory::Wall => "Wall",
        MaterialCategory::Door => "Door",
        MaterialCategory::Window => "Window",
        MaterialCategory::Shelf => "Rack / shelf",
        MaterialCategory::Slab => "Floor slab",
    }
}

const CATEGORIES: [MaterialCategory; 5] = [MaterialCategory::Wall, MaterialCategory::Door, MaterialCategory::Window, MaterialCategory::Shelf, MaterialCategory::Slab];

pub(super) fn window(p: &mut Planning, s: &mut Survey, ctx: &egui::Context) {
    if p.material_editor.is_none() {
        return;
    }
    let mut open = true;
    egui::Window::new("Materials").open(&mut open).default_width(420.0).show(ctx, |ui| {
        list(p, s, ui);
        ui.separator();
        form(p, s, ui);
    });
    if !open {
        p.material_editor = None;
    }
}

fn list(p: &mut Planning, s: &Survey, ui: &mut egui::Ui) {
    let data = &s.zone().planning;
    ui.strong("Custom materials of this level");
    if data.materials.is_empty() {
        ui.weak("None yet. Create one, or copy a preset to adjust it.");
    }
    let editing = p.material_editor.as_ref().and_then(|e| e.editing.clone());
    for m in &data.materials {
        if ui.selectable_label(editing.as_deref() == Some(m.id.as_str()), format!("{} ({})", m.name, category_label(m.category))).clicked() {
            p.material_editor = Some(MaterialEditor { editing: Some(m.id.clone()), draft: MaterialDraft::from_material(m), error: None });
        }
    }
    ui.horizontal(|ui| {
        if ui.button("New").clicked() {
            p.material_editor = Some(MaterialEditor { editing: None, draft: MaterialDraft::new_wall(), error: None });
        }
        egui::ComboBox::from_id_salt("copy_preset").selected_text("Copy a preset…").show_ui(ui, |ui| {
            for preset in &p.catalog.materials {
                if ui.selectable_label(false, &preset.name).clicked() {
                    p.material_editor = Some(MaterialEditor { editing: None, draft: MaterialDraft::copy_of(preset), error: None });
                }
            }
        });
    });
    ui.weak(format!("Presets (catalog v{}): {}", p.catalog.version, p.catalog.provenance));
}

fn form(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    let Some(editor) = p.material_editor.as_mut() else { return };
    ui.strong(if editor.editing.is_some() { "Edit material" } else { "New material" });
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut editor.draft.name);
    });
    ui.horizontal(|ui| {
        ui.label("Used for");
        egui::ComboBox::from_id_salt("material_category").selected_text(category_label(editor.draft.category)).show_ui(ui, |ui| {
            for c in CATEGORIES {
                ui.selectable_value(&mut editor.draft.category, c, category_label(c));
            }
        });
    });
    egui::Grid::new("material_values").num_columns(4).show(ui, |ui| {
        ui.label("");
        for b in Band::ALL {
            ui.label(b.label());
        }
        ui.end_row();
        ui.label("Loss per crossing");
        let a = &mut editor.draft.absorption_db;
        ui.add(egui::DragValue::new(&mut a.ghz2_4).range(0.0..=materials::MAX_ABSORPTION_DB).suffix(" dB"));
        ui.add(egui::DragValue::new(&mut a.ghz5).range(0.0..=materials::MAX_ABSORPTION_DB).suffix(" dB"));
        ui.add(egui::DragValue::new(&mut a.ghz6).range(0.0..=materials::MAX_ABSORPTION_DB).suffix(" dB"));
        ui.end_row();
        ui.label("Reflection");
        let r = &mut editor.draft.reflection;
        ui.add(egui::DragValue::new(&mut r.ghz2_4).range(0.0..=1.0).speed(0.01));
        ui.add(egui::DragValue::new(&mut r.ghz5).range(0.0..=1.0).speed(0.01));
        ui.add(egui::DragValue::new(&mut r.ghz6).range(0.0..=1.0).speed(0.01));
        ui.end_row();
    });
    if let Some(e) = &editor.error {
        ui.colored_label(egui::Color32::LIGHT_RED, e);
    }
    let editing = editor.editing.clone();
    let zone = s.zone;
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            p.save_material(&mut s.project, zone);
        }
        if let Some(id) = &editing {
            if ui.button("Copy to other floors").on_hover_text("Custom materials belong to one level; walls on other floors need their own copy").clicked() {
                p.copy_material_to_building(&mut s.project, zone, id);
            }
            if ui.button("Delete").clicked() {
                p.delete_material(&mut s.project, zone, id);
            }
        }
    });
}
