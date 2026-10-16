//! Side panel: properties of the selection, the wall list and drawing defaults.

use eframe::egui;
use openspot_model::{MaterialCategory, OpeningKind, PlanningData, Uuid, Wall, WallKind};

use super::materials::picker;
use crate::planning::geometry::distance;
use crate::planning::materials::{category_for, display_name};
use crate::planning::ops::Selection;
use crate::planning::tools::{DEFAULT_DOOR_WIDTH_M, DEFAULT_WINDOW_WIDTH_M, THICKNESS_PRESETS_M};
use crate::planning::{Planning, tools::Tool};
use crate::survey::Survey;

pub(super) fn panel(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        super::results::panel(p, s, ui);
        ui.separator();
        ui.heading("Selection");
        selection(p, s, ui);
        ui.separator();
        super::ap::model_picker(p, ui);
        ui.separator();
        defaults(p, s, ui);
        ui.separator();
        super::level::panel(p, s, ui);
        ui.separator();
        ap_list(p, s, ui);
        ui.separator();
        wall_list(p, s, ui);
    });
}

fn kind_label(kind: WallKind) -> &'static str {
    match kind {
        WallKind::Wall => "Wall",
        WallKind::Door => "Door (full segment)",
        WallKind::Window => "Window (full segment)",
        WallKind::Shelf => "Rack / shelf",
    }
}

fn selection(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui) {
    let Some(sel) = p.selection() else {
        ui.weak("Nothing selected. Click an element with the Select tool.");
        return;
    };
    let zone = s.zone;
    let data = s.zone().planning.clone();
    match sel {
        Selection::Wall(id) => {
            let Some(w) = data.walls.iter().find(|w| w.id == id) else { return };
            wall_props(p, s, ui, &data, w, zone);
        }
        Selection::Opening { wall, opening } => {
            let Some((w, o)) = data.walls.iter().find(|w| w.id == wall).and_then(|w| w.openings.iter().find(|o| o.id == opening).map(|o| (w, o))) else { return };
            let kind = if o.kind == OpeningKind::Door { "Door" } else { "Window" };
            ui.label(format!("{kind}, {:.2} m wide, in a {:.2} m wall", (o.end - o.start) * w.length_m(), w.length_m()));
            let category = if o.kind == OpeningKind::Door { MaterialCategory::Door } else { MaterialCategory::Window };
            if let Some(m) = picker(ui, "sel_opening_material", &data, &p.catalog, category, &o.material) {
                p.edit_wall(&mut s.project, zone, wall, |w| w.openings.iter_mut().filter(|x| x.id == opening).for_each(|x| x.material = m.clone()));
            }
            if ui.button("Delete").clicked() {
                p.delete_selection(&mut s.project, zone, 0.0);
            }
        }
        Selection::Ap(id) => super::ap::settings(p, s, ui, id),
        Selection::Cutout(id) => {
            let Some(c) = data.cutouts.iter().find(|c| c.id == id) else { return };
            ui.label(format!("Stairwell opening with {} corners. Radio signal passes through the floor slab here without slab loss.", c.polygon.len()));
            if ui.button("Delete").clicked() {
                p.delete_selection(&mut s.project, zone, 0.0);
            }
        }
    }
}

fn wall_props(p: &mut Planning, s: &mut Survey, ui: &mut egui::Ui, data: &PlanningData, w: &Wall, zone: usize) {
    ui.label(format!("{}, {:.2} m long", kind_label(w.kind), w.length_m()));
    ui.horizontal(|ui| {
        ui.label("Thickness");
        for (label, m) in THICKNESS_PRESETS_M {
            if ui.selectable_label((w.thickness_m - m).abs() < 1e-9, label).clicked() {
                p.edit_wall(&mut s.project, zone, w.id, |x| x.thickness_m = m);
            }
        }
    });
    if let Some(m) = picker(ui, "sel_wall_material", data, &p.catalog, category_for(w.kind), &w.material) {
        p.edit_wall(&mut s.project, zone, w.id, |x| x.material = m.clone());
    }
    if !w.openings.is_empty() {
        ui.weak(format!("{} door/window opening(s)", w.openings.len()));
    }
    if ui.button("Delete").clicked() {
        p.delete_selection(&mut s.project, zone, 0.0);
    }
}

/// Materials and sizes used for what is drawn next.
fn defaults(p: &mut Planning, s: &Survey, ui: &mut egui::Ui) {
    ui.heading("New elements");
    let data = &s.zone().planning;
    egui::Grid::new("planning_defaults").num_columns(2).show(ui, |ui| {
        for (label, id, category, field) in [
            ("Rack", "def_shelf", MaterialCategory::Shelf, &mut p.settings.shelf_material),
            ("Door", "def_door", MaterialCategory::Door, &mut p.settings.door_material),
            ("Window", "def_window", MaterialCategory::Window, &mut p.settings.window_material),
        ] {
            ui.label(label);
            if let Some(m) = picker(ui, id, data, &p.catalog, category, field) {
                *field = m;
            }
            ui.end_row();
        }
        ui.label("Door width");
        ui.add(egui::DragValue::new(&mut p.settings.door_width_m).range(0.4..=3.0).speed(0.01).suffix(" m"));
        ui.end_row();
        ui.label("Window width");
        ui.add(egui::DragValue::new(&mut p.settings.window_width_m).range(0.3..=6.0).speed(0.01).suffix(" m"));
        ui.end_row();
    });
    if ui.small_button("Reset widths").clicked() {
        (p.settings.door_width_m, p.settings.window_width_m) = (DEFAULT_DOOR_WIDTH_M, DEFAULT_WINDOW_WIDTH_M);
    }
}

fn wall_list(p: &mut Planning, s: &Survey, ui: &mut egui::Ui) {
    let data = &s.zone().planning;
    ui.heading(format!("Walls ({})", data.walls.len()));
    if data.walls.is_empty() {
        ui.weak("Draw walls with the Wall tool.");
    }
    let selected: Option<Uuid> = match p.selection() {
        Some(Selection::Wall(id)) => Some(id),
        Some(Selection::Opening { wall, .. }) => Some(wall),
        _ => None,
    };
    for (i, w) in data.walls.iter().enumerate() {
        let len = distance(w.a, w.b);
        let label = format!("{}. {} · {:.2} m · {}", i + 1, kind_label(w.kind), len, display_name(data, &p.catalog, &w.material));
        if ui.selectable_label(selected == Some(w.id), label).clicked() {
            p.editor.selection = Some(Selection::Wall(w.id));
            p.set_tool(Tool::Select);
        }
    }
}

fn ap_list(p: &mut Planning, s: &Survey, ui: &mut egui::Ui) {
    let data = &s.zone().planning;
    ui.heading(format!("Access points ({})", data.access_points.len()));
    if data.access_points.is_empty() {
        ui.weak("Place access points with the Access point tool.");
    }
    for ap in &data.access_points {
        if ui.selectable_label(p.selection() == Some(Selection::Ap(ap.id)), super::ap::summary(ap)).clicked() {
            p.editor.selection = Some(Selection::Ap(ap.id));
            p.set_tool(Tool::Select);
        }
    }
}
