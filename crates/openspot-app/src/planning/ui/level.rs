//! Level panel: where this zone sits in its building and the floor slab below it.
//! UNVERIFIED: egui layer written without a display.

use eframe::egui;
use openspot_model::MaterialCategory;

use super::materials::picker;
use crate::planning::Planning;
use crate::planning::levels::LevelStack;
use crate::survey::Survey;

/// Level selector: every zone grouped by building, plus floor-up / floor-down steps.
pub(super) fn selector(s: &mut Survey, ui: &mut egui::Ui) {
    let outline = s.project.outline();
    let current = s.zone().name.clone();
    let mut chosen = None;
    egui::ComboBox::from_id_salt("planning_level").width(180.0).selected_text(format!("Level: {current}")).show_ui(ui, |ui| {
        for b in &outline.buildings {
            ui.weak(&b.name);
            for z in &b.zones {
                let label = z.level.as_ref().map_or_else(|| z.name.clone(), |l| format!("{l}: {}", z.name));
                if ui.selectable_label(z.zone_index == s.zone, label).clicked() {
                    chosen = Some(z.zone_index);
                }
            }
        }
        if !outline.ungrouped.is_empty() {
            ui.weak("Not in a building");
        }
        for z in &outline.ungrouped {
            if ui.selectable_label(z.zone_index == s.zone, &z.name).clicked() {
                chosen = Some(z.zone_index);
            }
        }
    });
    for (label, delta) in [("▲", 1), ("▼", -1)] {
        let target = LevelStack::neighbor(&s.project, s.zone, delta);
        if ui.add_enabled(target.is_some(), egui::Button::new(label).small()).on_hover_text(if delta > 0 { "Floor above" } else { "Floor below" }).clicked() {
            chosen = target;
        }
    }
    if let Some(i) = chosen {
        s.select_zone(i);
    }
}

pub(super) fn panel(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    ui.heading("Level");
    let zone_id = s.zone().id;
    let zone = s.zone;
    match s.project.structure.building_of(zone_id) {
        Some(b) => {
            let building = &s.project.structure.buildings[b];
            let index = building.floors.iter().position(|f| f.zone == zone_id).unwrap_or(0);
            let elevation = s.project.structure.elevation_m(building.id, index).unwrap_or(0.0);
            let label = building.floors.get(index).map_or("", |f| f.label.as_str());
            ui.label(format!("{} · {} (floor {} of {}) · floor at {:.1} m", building.name, label, index + 1, building.floors.len(), elevation));
        }
        None => {
            ui.weak("This zone is not part of a building, so only this floor is predicted. Group floors as a building in the Survey explorer to include the floors above and below.");
        }
    }
    let data = &s.zone().planning;
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label("Floor slab below");
        picked = picker(ui, "slab_material", data, &p.catalog, MaterialCategory::Slab, &data.level.slab.material);
    });
    if let Some(m) = picked {
        p.set_slab_material(&mut s.project, zone, &m);
    }
    ui.checkbox(&mut p.show_floor_below, "Show the floor below faintly");
    ui.weak("The slab's loss applies to signals crossing between this floor and the one below, except through a stairwell opening.");
}
