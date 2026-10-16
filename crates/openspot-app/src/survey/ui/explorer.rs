//! Survey UI: project explorer tree (project > building > zone > snapshot).

use eframe::egui;
use openspot_model::{Outline, Uuid, ZoneNode};

use crate::survey::Survey;

/// What a click in the tree asks for; applied after drawing so the tree is not mutated mid-walk.
enum Action {
    Select(usize, Option<usize>),
    Move(usize, Uuid),
    Ungroup(usize),
    Shift(usize, isize),
    RemoveBuilding(Uuid),
    BeginRename(Uuid, String),
}

pub(super) fn panel(s: &mut Survey, ui: &mut egui::Ui) {
    ui.heading("Explorer");
    let outline = s.project.outline();
    let names: Vec<(Uuid, String)> = outline.buildings.iter().map(|b| (b.id, b.name.clone())).collect();
    let mut actions = Vec::new();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(egui::RichText::new(&s.project.name).strong());
        draw_outline(s, ui, &outline, &names, &mut actions);
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut s.explorer.new_building).hint_text("New building").desired_width(110.0));
            if ui.button("Add").clicked() {
                let name = std::mem::take(&mut s.explorer.new_building);
                s.add_building(&name);
            }
        });
    });
    for action in actions {
        match action {
            Action::Select(z, snap) => s.select_node(z, snap),
            Action::Move(z, b) => s.move_zone_to_building(z, b),
            Action::Ungroup(z) => s.ungroup_zone(z),
            Action::Shift(z, d) => s.shift_floor(z, d),
            Action::RemoveBuilding(b) => s.remove_building(b),
            Action::BeginRename(b, n) => s.explorer.rename = Some((b, n)),
        }
    }
}

fn draw_outline(s: &mut Survey, ui: &mut egui::Ui, outline: &Outline, buildings: &[(Uuid, String)], actions: &mut Vec<Action>) {
    for b in &outline.buildings {
        if let Some((id, text)) = s.explorer.rename.as_mut().filter(|(id, _)| *id == b.id) {
            let field = ui.add(egui::TextEdit::singleline(text).desired_width(150.0));
            field.request_focus();
            if field.lost_focus() {
                let (id, text) = (*id, text.clone());
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    s.rename_building(id, &text);
                }
                s.explorer.rename = None;
            }
            continue;
        }
        egui::CollapsingHeader::new(format!("🏢 {}", b.name)).id_salt(b.id).default_open(true).show(ui, |ui| {
            for z in &b.zones {
                zone_node(s, ui, z, buildings, actions);
            }
        })
        .header_response
        .context_menu(|ui| {
            if ui.button("Rename…").clicked() {
                actions.push(Action::BeginRename(b.id, b.name.clone()));
                ui.close();
            }
            if ui.button("Delete building (keeps zones)").clicked() {
                actions.push(Action::RemoveBuilding(b.id));
                ui.close();
            }
        });
    }
    for z in &outline.ungrouped {
        zone_node(s, ui, z, buildings, actions);
    }
}

fn zone_node(s: &Survey, ui: &mut egui::Ui, z: &ZoneNode, buildings: &[(Uuid, String)], actions: &mut Vec<Action>) {
    let title = match &z.level {
        Some(level) => format!("{level} · {}", z.name),
        None => z.name.clone(),
    };
    let header = egui::CollapsingHeader::new(title).id_salt(("zone", z.zone_index)).default_open(z.zone_index == s.zone).show(ui, |ui| {
        for (i, name) in &z.snapshots {
            let selected = z.zone_index == s.zone && *i == s.snapshot;
            if ui.selectable_label(selected, name).clicked() {
                actions.push(Action::Select(z.zone_index, Some(*i)));
            }
        }
    });
    if header.header_response.clicked() {
        actions.push(Action::Select(z.zone_index, None));
    }
    header.header_response.context_menu(|ui| {
        ui.menu_button("Move to building", |ui| {
            for (id, name) in buildings {
                if ui.button(name).clicked() {
                    actions.push(Action::Move(z.zone_index, *id));
                    ui.close();
                }
            }
            if buildings.is_empty() {
                ui.label("No buildings yet");
            }
        });
        if z.level.is_some() {
            if ui.button("Floor up").clicked() {
                actions.push(Action::Shift(z.zone_index, 1));
                ui.close();
            }
            if ui.button("Floor down").clicked() {
                actions.push(Action::Shift(z.zone_index, -1));
                ui.close();
            }
            if ui.button("Remove from building").clicked() {
                actions.push(Action::Ungroup(z.zone_index));
                ui.close();
            }
        }
    });
}
