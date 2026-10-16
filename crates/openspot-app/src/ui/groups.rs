//! Custom group controls: the assign menu for a table row and the group manager window.

use eframe::egui::{self, Align2, Vec2};
use openspot_wlan::MacAddress;

use super::dialog::escape_pressed;
use crate::groups::CustomGroups;

#[derive(Debug, Default)]
pub struct GroupsUi {
    manager_open: bool,
    /// New-group dialog: the APs to put in the group once it exists, and the name being typed.
    new_group: Option<(Vec<MacAddress>, String)>,
    /// Rename in progress: the group's current name and the new text.
    renaming: Option<(String, String)>,
    /// Last validation message for whichever dialog is open.
    error: Option<String>,
}

impl GroupsUi {
    pub fn open_manager(&mut self) {
        self.manager_open = true;
    }

    /// "Add to group" submenu for the right-clicked row; `targets` are all APs it applies to.
    pub fn assign_menu(&mut self, ui: &mut egui::Ui, groups: &mut CustomGroups, targets: &[MacAddress]) {
        ui.menu_button("Group", |ui| {
            let current = targets.first().and_then(|t| groups.group_of(t)).map(str::to_owned);
            let names: Vec<String> = groups.iter().map(|g| g.name.clone()).collect();
            for name in names {
                let mark = if current.as_deref() == Some(name.as_str()) { "✔ " } else { "" };
                if ui.button(format!("{mark}{name}")).clicked() {
                    for t in targets {
                        // The group was listed a moment ago, so it still exists.
                        let _ = groups.assign(t, Some(&name));
                    }
                    ui.close();
                }
            }
            if !groups.is_empty() {
                ui.separator();
            }
            if ui.button("New group…").clicked() {
                self.new_group = Some((targets.to_vec(), String::new()));
                self.error = None;
                ui.close();
            }
            if ui.button("Remove from group").clicked() {
                for t in targets {
                    let _ = groups.assign(t, None);
                }
                ui.close();
            }
        });
    }

    pub fn windows(&mut self, ctx: &egui::Context, groups: &mut CustomGroups) {
        self.new_group_dialog(ctx, groups);
        self.manager_window(ctx, groups);
    }

    fn new_group_dialog(&mut self, ctx: &egui::Context, groups: &mut CustomGroups) {
        let Some((targets, text)) = self.new_group.as_mut() else { return };
        let (mut create, mut cancel) = (false, escape_pressed(ctx));
        egui::Window::new("New group")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let field = ui.add(egui::TextEdit::singleline(text).hint_text("Group name").desired_width(240.0));
                field.request_focus();
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(e) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, e);
                }
                ui.horizontal(|ui| {
                    create = ui.button("Create").clicked() || enter;
                    cancel |= ui.button("Cancel").clicked();
                });
            });
        if create {
            match groups.create(text) {
                Ok(name) => {
                    for t in targets.iter() {
                        let _ = groups.assign(t, Some(&name));
                    }
                    self.new_group = None;
                    self.error = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        } else if cancel {
            self.new_group = None;
            self.error = None;
        }
    }

    fn manager_window(&mut self, ctx: &egui::Context, groups: &mut CustomGroups) {
        if !self.manager_open {
            return;
        }
        let mut open = true;
        let mut delete: Option<String> = None;
        let mut start_rename: Option<(String, String)> = None;
        let mut commit_rename = false;
        let mut cancel_rename = false;
        egui::Window::new("Custom groups")
            .open(&mut open)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                if groups.is_empty() {
                    ui.label("No groups yet. Right-click a network and choose Group, then New group.");
                }
                for g in groups.iter() {
                    ui.horizontal(|ui| match &mut self.renaming {
                        Some((old, text)) if *old == g.name => {
                            let field = ui.text_edit_singleline(text);
                            field.request_focus();
                            commit_rename = ui.button("Save").clicked() || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                            cancel_rename = ui.button("Cancel").clicked();
                        }
                        _ => {
                            ui.label(format!("{} ({} networks)", g.name, g.members.len()));
                            if ui.button("Rename").clicked() {
                                start_rename = Some((g.name.clone(), g.name.clone()));
                            }
                            if ui.button("Delete").clicked() {
                                delete = Some(g.name.clone());
                            }
                        }
                    });
                }
                if let Some(e) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, e);
                }
            });
        if let Some(r) = start_rename {
            self.renaming = Some(r);
            self.error = None;
        }
        if let Some(name) = delete {
            let _ = groups.delete(&name);
        }
        if commit_rename && let Some((old, new)) = self.renaming.clone() {
            match groups.rename(&old, &new) {
                Ok(()) => self.renaming = None,
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        if cancel_rename || (escape_pressed(ctx) && self.renaming.is_some()) {
            self.renaming = None;
            self.error = None;
        } else if escape_pressed(ctx) {
            open = false;
        }
        if !open {
            self.manager_open = false;
            self.renaming = None;
            self.error = None;
        }
    }
}
