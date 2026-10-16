//! Survey UI: start page with New / Open and the sortable recent-projects list.

use std::path::PathBuf;

use eframe::egui;

use crate::survey::Survey;
use crate::survey::projects::{self, SortKey, format_utc};

use super::toolbar::{pick_file, report};

pub(super) fn page(s: &mut Survey, ctx: &egui::Context) {
    let mut open: Option<PathBuf> = None;
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("OpenSpot");
        ui.label("Wi-Fi site surveys");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("New project").clicked() {
                s.new_project();
            }
            if ui.button("Open…").clicked() {
                open = pick_file("OpenSpot project", &["ospot"]);
            }
            if ui.button("Continue current").on_hover_text("Back to the project you were working on").clicked() {
                s.explorer.start_page = false;
            }
        });
        ui.separator();
        if s.explorer.recent.is_empty() {
            ui.label("No recent projects yet.");
            return;
        }
        let rows = s.explorer.rows.get_or_insert_with(|| projects::load_rows(&s.explorer.recent));
        projects::sort_rows(rows, s.explorer.sort, s.explorer.ascending);
        let mut clicked_sort = None;
        egui::Grid::new("recent_projects").striped(true).num_columns(5).show(ui, |ui| {
            for key in SortKey::ALL {
                let arrow = if key != s.explorer.sort { "" } else if s.explorer.ascending { " ▲" } else { " ▼" };
                let label = if matches!(key, SortKey::Created | SortKey::Changed | SortKey::LastOpened) {
                    format!("{} (UTC){arrow}", key.label())
                } else {
                    format!("{}{arrow}", key.label())
                };
                if ui.button(label).clicked() {
                    clicked_sort = Some(key);
                }
            }
            ui.end_row();
            for r in rows.iter() {
                if ui.link(&r.name).on_hover_text(r.path.display().to_string()).clicked() {
                    open = Some(r.path.clone());
                }
                ui.label(format_utc(r.created));
                ui.label(format_utc(r.changed));
                ui.label(format_utc(r.last_opened));
                ui.label(r.zone_count.to_string());
                ui.end_row();
            }
        });
        if let Some(key) = clicked_sort {
            // Clicking the active column flips the direction; a new column starts in its natural one.
            if key == s.explorer.sort {
                s.explorer.ascending = !s.explorer.ascending;
            } else {
                s.explorer.sort = key;
                s.explorer.ascending = key.starts_ascending();
            }
        }
    });
    if let Some(path) = open {
        let r = s.open_from(&path);
        report(s, r, "Project opened");
    }
}
