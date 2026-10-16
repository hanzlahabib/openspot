//! Alias import/export controls: a menu plus the import preview window.

use std::fs;

use eframe::egui::{self, Align2, Vec2};

use super::dialog::escape_pressed;
use crate::alias_csv::{self, ConflictPolicy, ImportMode, ParsedAliases};
use crate::aliases::Aliases;

/// Skipped lines listed in the preview before the rest are summarised.
const MAX_LISTED_SKIPS: usize = 8;
const DEFAULT_EXPORT_NAME: &str = "openspot-aliases.csv";

#[derive(Debug)]
pub struct AliasIo {
    pending: Option<ParsedAliases>,
    mode: ImportMode,
    policy: ConflictPolicy,
    confirm_forget: bool,
    /// Result of the last operation, for the caller to show.
    pub notice: Option<String>,
}

impl Default for AliasIo {
    fn default() -> Self {
        Self { pending: None, mode: ImportMode::Lenient, policy: ConflictPolicy::default(), confirm_forget: false, notice: None }
    }
}

impl AliasIo {
    /// Import, export and "forget all" buttons; also used inside the preferences window.
    pub fn buttons(&mut self, ui: &mut egui::Ui, aliases: &Aliases) {
        let mut strict = self.mode == ImportMode::Strict;
        if ui.checkbox(&mut strict, "Strict import (reject the whole file on any bad line)").changed() {
            self.mode = if strict { ImportMode::Strict } else { ImportMode::Lenient };
        }
        if ui.button("Import aliases from CSV…").clicked() {
            self.import_dialog();
            ui.close();
        }
        if ui.add_enabled(!aliases.is_empty(), egui::Button::new("Export aliases to CSV…")).clicked() {
            self.export_dialog(aliases);
            ui.close();
        }
        if ui.add_enabled(!aliases.is_empty(), egui::Button::new("Forget all aliases…")).clicked() {
            self.confirm_forget = true;
            ui.close();
        }
    }

    fn import_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file() else { return };
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.notice = Some(format!("could not read {}: {e}", path.display()));
                return;
            }
        };
        match alias_csv::parse(&text, self.mode) {
            Ok(parsed) => self.pending = Some(parsed),
            Err(e) => self.notice = Some(format!("alias import failed, nothing changed: {e}")),
        }
    }

    fn export_dialog(&mut self, aliases: &Aliases) {
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(DEFAULT_EXPORT_NAME).save_file() else { return };
        self.notice = Some(match fs::write(&path, alias_csv::export(aliases)) {
            Ok(()) => format!("exported {} {} aliases to {}", aliases.len(), aliases.scope().label(), path.display()),
            Err(e) => format!("could not write {}: {e}", path.display()),
        });
    }

    /// The import preview and the forget-all confirmation, when open.
    pub fn windows(&mut self, ctx: &egui::Context, aliases: &mut Aliases) {
        self.import_preview(ctx, aliases);
        self.forget_confirmation(ctx, aliases);
    }

    fn import_preview(&mut self, ctx: &egui::Context, aliases: &mut Aliases) {
        let Some(parsed) = &self.pending else { return };
        let (mut accept, mut cancel) = (false, escape_pressed(ctx));
        egui::Window::new("Import aliases")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("{} aliases found, {} lines skipped. They are added to the {} aliases.", parsed.entries.len(), parsed.skipped.len(), aliases.scope().label()));
                for s in parsed.skipped.iter().take(MAX_LISTED_SKIPS) {
                    ui.weak(format!("line {}: {}", s.line, s.reason));
                }
                if parsed.skipped.len() > MAX_LISTED_SKIPS {
                    ui.weak(format!("… and {} more", parsed.skipped.len() - MAX_LISTED_SKIPS));
                }
                ui.separator();
                ui.label("If a BSSID already has a different alias:");
                ui.radio_value(&mut self.policy, ConflictPolicy::Overwrite, "Use the imported alias");
                ui.radio_value(&mut self.policy, ConflictPolicy::KeepExisting, "Keep my existing alias");
                ui.horizontal(|ui| {
                    accept = ui.add_enabled(!parsed.entries.is_empty(), egui::Button::new("Import")).clicked();
                    cancel |= ui.button("Cancel").clicked();
                });
            });
        if accept {
            if let Some(parsed) = self.pending.take() {
                self.notice = Some(format!("{} aliases: {}", aliases.scope().label(), alias_csv::apply(parsed, aliases, self.policy).summary()));
            }
        } else if cancel {
            self.pending = None;
        }
    }

    fn forget_confirmation(&mut self, ctx: &egui::Context, aliases: &mut Aliases) {
        if !self.confirm_forget {
            return;
        }
        let (mut confirm, mut cancel) = (false, escape_pressed(ctx));
        egui::Window::new("Forget all aliases?")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("This removes all {} {} aliases. Export them first if you may need them again.", aliases.len(), aliases.scope().label()));
                ui.horizontal(|ui| {
                    confirm = ui.button("Forget all").clicked();
                    cancel |= ui.button("Cancel").clicked();
                });
            });
        if confirm {
            let n = aliases.len();
            aliases.clear();
            self.notice = Some(format!("removed {n} {} aliases", aliases.scope().label()));
        }
        if confirm || cancel {
            self.confirm_forget = false;
        }
    }
}
