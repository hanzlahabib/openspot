use std::collections::BTreeSet;

use eframe::egui::{self, RichText, Sense};
use egui_extras::{Column as TableColumn, TableBuilder};
use openspot_wlan::MacAddress;

use super::color_for;
use crate::aliases::Aliases;
use crate::columns::{self, Column, ColumnSet};
use crate::grouping::{Entry, GroupBy, flatten, group_rows};
use crate::groups::CustomGroups;
use crate::ui::groups::GroupsUi;
use crate::state::{Sort, Track};

const ROW_HEIGHT: f32 = 22.0;
const HEADER_HEIGHT: f32 = 24.0;
/// The last column absorbs the remaining width but never shrinks below this.
const LAST_COLUMN_MIN_WIDTH: f32 = 100.0;

/// Everything the table reads or mutates, bundled so call sites stay readable.
pub struct TableView<'a> {
    pub sort: &'a mut Sort,
    pub selected: &'a mut BTreeSet<MacAddress>,
    pub aliases: &'a mut Aliases,
    pub groups: &'a mut CustomGroups,
    pub groups_ui: &'a mut GroupsUi,
    /// Set to ask the caller to open the alias editor for this BSSID with this text.
    pub alias_request: &'a mut Option<(MacAddress, String)>,
    pub columns: &'a ColumnSet,
    pub group_by: GroupBy,
    /// Titles of collapsed groups.
    pub collapsed: &'a mut BTreeSet<String>,
    pub now: f64,
}

/// Renders the AP table. Clicking a row toggles it in `selected`; clicking a header
/// changes `sort`; clicking a group header collapses it; right-click opens the alias menu.
pub fn show(ui: &mut egui::Ui, rows: &[&Track], view: &mut TableView<'_>) {
    let grouped = group_rows(rows, view.group_by, |bssid| view.groups.group_of(bssid).map(str::to_owned));
    let entries = flatten(&grouped, view.group_by, view.collapsed);
    let visible: Vec<Column> = view.columns.iter().collect();

    let mut builder = TableBuilder::new(ui).striped(true).sense(Sense::click()).resizable(true);
    for (i, c) in visible.iter().enumerate() {
        builder = if i + 1 == visible.len() {
            builder.column(TableColumn::remainder().at_least(LAST_COLUMN_MIN_WIDTH))
        } else {
            builder.column(TableColumn::initial(c.width()).at_least(30.0))
        };
    }

    builder
        .header(HEADER_HEIGHT, |mut head| {
            for &c in &visible {
                head.col(|ui| header_cell(ui, view.sort, c));
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, entries.len(), |mut row| match &entries[row.index()] {
                Entry::Header { title, count, collapsed } => {
                    row.col(|ui| {
                        let arrow = if *collapsed { "▶" } else { "▼" };
                        ui.label(RichText::new(format!("{arrow} {title}")).strong());
                    });
                    row.col(|ui| {
                        ui.weak(format!("{count} network{}", if *count == 1 { "" } else { "s" }));
                    });
                    if row.response().clicked() && !view.collapsed.remove(title) {
                        view.collapsed.insert(title.clone());
                    }
                }
                Entry::Row(track) => data_row(&mut row, track, &visible, view),
            });
        });
}

fn header_cell(ui: &mut egui::Ui, sort: &mut Sort, c: Column) {
    let key = c.sort_key();
    let arrow = match key {
        Some(k) if k == sort.key => if sort.descending { " ▼" } else { " ▲" },
        _ => "",
    };
    let text = RichText::new(format!("{}{arrow}", c.title())).strong();
    match key {
        Some(k) => {
            if ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_text(c.description()).clicked() {
                sort.toggle(k);
            }
        }
        None => {
            ui.label(text).on_hover_text(c.description());
        }
    }
}

fn data_row(row: &mut egui_extras::TableRow<'_, '_>, track: &Track, visible: &[Column], view: &mut TableView<'_>) {
    let ap = &track.ap;
    row.set_selected(view.selected.contains(&ap.bssid));
    for &c in visible {
        row.col(|ui| match c {
            Column::Ssid => {
                ui.colored_label(color_for(&ap.bssid), "●");
                ui.label(view.aliases.name_for(&ap.bssid, &ap.ssid));
                if view.aliases.get(&ap.bssid).is_some() && !ap.ssid.is_empty() {
                    ui.weak(format!("({})", ap.ssid));
                }
            }
            Column::Bssid => {
                ui.monospace(ap.bssid.to_string());
            }
            Column::Seen => {
                let age = view.now - track.first_seen;
                let text = columns::raw_value(c, track, view.aliases, view.now);
                ui.label(text).on_hover_text(match columns::age_label(age).as_str() {
                    "now" => "first seen just now".to_owned(),
                    ago => format!("first seen {ago} ago"),
                });
            }
            _ => {
                ui.label(columns::raw_value(c, track, view.aliases, view.now));
            }
        });
    }
    let response = row.response();
    if response.clicked() && !view.selected.remove(&ap.bssid) {
        view.selected.insert(ap.bssid);
    }
    response.context_menu(|ui| {
        if ui.button("Set alias…").clicked() {
            *view.alias_request = Some((ap.bssid, view.aliases.get(&ap.bssid).unwrap_or_default().to_owned()));
            ui.close();
        }
        if view.aliases.get(&ap.bssid).is_some() && ui.button("Remove alias").clicked() {
            view.aliases.set(&ap.bssid, "");
            ui.close();
        }
        // Acting on a selected row applies to the whole selection, like most list UIs.
        let targets: Vec<MacAddress> =
            if view.selected.contains(&ap.bssid) { view.selected.iter().copied().collect() } else { vec![ap.bssid] };
        view.groups_ui.assign_menu(ui, view.groups, &targets);
    });
}
