//! Survey UI: dialogs.

use eframe::egui::{self, Align2, Vec2};
use crate::survey::{ManageAction, Calibration, Survey};

pub(super) fn calibration_dialog(s: &mut Survey, ctx: &egui::Context) {
    let Calibration::Enter { a, b, .. } = s.calibration else { return };
    let pixels = (a.0 - b.0).hypot(a.1 - b.1);
    let mut apply = false;
    let mut cancel = false;
    egui::Window::new("Set scale").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.label(format!("The two points are {pixels:.0} px apart. How far apart are they in real life?"));
        if let Calibration::Enter { meters, .. } = &mut s.calibration {
            let unit = s.unit;
            let mut shown = unit.from_meters(*meters);
            ui.horizontal(|ui| {
                let range = unit.from_meters(0.05)..=unit.from_meters(10_000.0);
                ui.add(egui::DragValue::new(&mut shown).speed(0.1).range(range).suffix(format!(" {}", unit.label())));
            });
            *meters = unit.to_meters(shown);
        }
        ui.horizontal(|ui| {
            apply = ui.button("Apply").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if apply {
        if let Some(scale) = s.calibration.confirm() {
            s.set_scale(scale);
            s.message = Some("Scale set".to_owned());
        }
    } else if cancel {
        s.calibration.cancel();
    }
}

pub(super) fn manage_dialog(s: &mut Survey, ctx: &egui::Context) {
    let Some(action) = s.manage.clone() else { return };
    let (mut confirm, mut cancel) = (false, false);
    let mut text = match &action {
        ManageAction::RenameZone(t) | ManageAction::RenameSnapshot(t) => t.clone(),
        _ => String::new(),
    };
    let title = match &action {
        ManageAction::RenameZone(_) => "Rename zone",
        ManageAction::RenameSnapshot(_) => "Rename snapshot",
        ManageAction::DeleteZone => "Delete zone",
        ManageAction::DeleteSnapshot => "Delete snapshot",
    };
    egui::Window::new(title).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        match &action {
            ManageAction::RenameZone(_) | ManageAction::RenameSnapshot(_) => {
                let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(260.0));
                field.request_focus();
                confirm = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            }
            ManageAction::DeleteZone => {
                ui.label(format!("Delete zone “{}” with all of its snapshots and measurements?", s.zone().name));
                ui.label("This cannot be undone.");
            }
            ManageAction::DeleteSnapshot => {
                ui.label(format!("Delete snapshot “{}” and its measurements?", s.snapshot().name));
                ui.label("This cannot be undone.");
            }
        }
        ui.horizontal(|ui| {
            let label = if matches!(action, ManageAction::DeleteZone | ManageAction::DeleteSnapshot) { "Delete" } else { "Rename" };
            confirm |= ui.button(label).clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if confirm {
        match action {
            ManageAction::RenameZone(_) => drop(s.rename_zone(&text)),
            ManageAction::RenameSnapshot(_) => drop(s.rename_snapshot(&text)),
            ManageAction::DeleteZone => drop(s.delete_zone()),
            ManageAction::DeleteSnapshot => drop(s.delete_snapshot()),
        }
        s.manage = None;
    } else if cancel {
        s.manage = None;
    } else if let Some(ManageAction::RenameZone(t) | ManageAction::RenameSnapshot(t)) = s.manage.as_mut() {
        *t = text;
    }
}

pub(super) fn blank_dialog(s: &mut Survey, ctx: &egui::Context) {
    let Some((w_m, h_m)) = s.blank_dialog else { return };
    let unit = s.unit;
    let (mut w, mut h) = (unit.from_meters(w_m), unit.from_meters(h_m));
    let (mut create, mut cancel) = (false, false);
    egui::Window::new("Blank plan").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.label("Size of the area you will walk. The scale is set automatically.");
        ui.horizontal(|ui| {
            ui.label("Width");
            let range = unit.from_meters(1.0)..=unit.from_meters(1000.0);
            ui.add(egui::DragValue::new(&mut w).speed(0.5).range(range.clone()).suffix(format!(" {}", unit.label())));
            ui.label("Height");
            ui.add(egui::DragValue::new(&mut h).speed(0.5).range(range).suffix(format!(" {}", unit.label())));
        });
        ui.horizontal(|ui| {
            create = ui.button("Create").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if create {
        let r = s.create_blank_map(unit.to_meters(w), unit.to_meters(h));
        super::toolbar::report(s, r, "Blank plan created. Click where you stand to measure.");
        s.blank_dialog = None;
    } else if cancel {
        s.blank_dialog = None;
    } else {
        s.blank_dialog = Some((unit.to_meters(w), unit.to_meters(h)));
    }
}
