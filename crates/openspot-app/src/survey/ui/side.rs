//! Survey UI: side.

use eframe::egui::{self, Color32};
use openspot_wlan::MacAddress;

use crate::groups::CustomGroups;
use crate::grouping::UNGROUPED_LABEL;
use crate::survey::network_groups::{self, Ticked};
use crate::survey::{GroupFilter, MetricChoice, Survey};
use crate::aliases::Aliases;
use crate::ui::color_for;

pub(super) fn legend(s: &mut Survey, ui: &mut egui::Ui) {
    let metric = s.metric();
    let Some(palette) = metric.palette() else {
        gradient_legend(s, ui);
        return;
    };
    ui.heading("Legend");
    let shares = s.textures.heat_grid.as_ref().map(|g| openspot_heatmap::category_shares(g, palette.entries().len()));
    for (i, (label, [r, g, b])) in palette.entries().iter().enumerate() {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(*r, *g, *b), "■");
            match shares.as_ref().and_then(|sh| sh.get(i)) {
                Some(share) => ui.label(format!("{label}  {:.0}%", share * 100.0)),
                None => ui.label(*label),
            };
        });
    }
    ui.separator();
}

/// Color bar with the metric's value range, for continuous heatmaps.
pub(super) fn gradient_legend(s: &mut Survey, ui: &mut egui::Ui) {
    let metric = s.metric();
    let gradient = openspot_heatmap::effective_gradient(&metric, s.gradient, s.active_range());
    ui.heading("Legend");
    crate::heat_ui::gradient_bar(ui, &gradient, metric.unit());
    super::range::inputs(s, ui);
    ui.separator();
}

pub(super) fn networks(s: &mut Survey, ui: &mut egui::Ui, aliases: &Aliases, groups: &CustomGroups) {
    legend(s, ui);
    ui.heading("Networks");
    let nets = s.snapshot().networks();
    if nets.is_empty() {
        ui.label("Networks appear here after the first measurement.");
        return;
    }
    s.group_filter = s.group_filter.resolved(groups);
    if !groups.is_empty() {
        group_filter_combo(s, ui, groups);
    }
    let visible = network_groups::sections(&nets, groups, &s.group_filter);
    ui.horizontal(|ui| {
        if ui.small_button("All").clicked() {
            network_groups::select_visible(&mut s.selected, &visible);
        }
        if ui.small_button("None").clicked() {
            network_groups::deselect_visible(&mut s.selected, &visible, &s.group_filter);
        }
    });
    if s.metric_choice != MetricChoice::SelectedSignal {
        ui.weak("Choose “Signal level (selected networks)” to map only the ticked ones.");
    }
    if groups.is_empty() {
        ui.weak("Group networks in Inspector (Groups…) to select them together here.");
    }
    super::per_ap::switcher(s, ui, aliases);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for section in &visible {
            if !groups.is_empty() {
                group_header(s, ui, section);
            }
            for (bssid, ssid, best) in &section.rows {
                network_row(s, ui, aliases, *bssid, ssid, *best);
            }
        }
    });
}

fn group_filter_combo(s: &mut Survey, ui: &mut egui::Ui, groups: &CustomGroups) {
    let label = match &s.group_filter {
        GroupFilter::All => "All networks".to_owned(),
        GroupFilter::Group(name) => name.clone(),
        GroupFilter::Ungrouped => UNGROUPED_LABEL.to_owned(),
    };
    ui.horizontal(|ui| {
        ui.label("Group:");
        egui::ComboBox::from_id_salt("survey_group_filter").selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(&mut s.group_filter, GroupFilter::All, "All networks");
            for g in groups.iter() {
                ui.selectable_value(&mut s.group_filter, GroupFilter::Group(g.name.clone()), &g.name);
            }
            ui.selectable_value(&mut s.group_filter, GroupFilter::Ungrouped, UNGROUPED_LABEL);
        });
    });
}

/// Header with a tri-state box that ticks or unticks the whole group.
fn group_header(s: &mut Survey, ui: &mut egui::Ui, section: &network_groups::Section) {
    let state = network_groups::ticked(&s.selected, section);
    let mut on = state == Ticked::All;
    let name = section.name.as_deref().unwrap_or(UNGROUPED_LABEL);
    let text = egui::RichText::new(format!("{name} ({})", section.rows.len())).strong();
    let checkbox = egui::Checkbox::new(&mut on, text).indeterminate(state == Ticked::Some);
    if ui.add(checkbox).on_hover_text("Select or deselect every network in this group").changed() {
        network_groups::set_section(&mut s.selected, section, on);
    }
}

fn network_row(s: &mut Survey, ui: &mut egui::Ui, aliases: &Aliases, bssid: MacAddress, ssid: &str, best: i32) {
    ui.horizontal(|ui| {
        let mut on = s.selected.contains(&bssid);
        if ui.checkbox(&mut on, "").changed() {
            if on {
                s.selected.insert(bssid);
            } else {
                s.selected.remove(&bssid);
            }
        }
        ui.colored_label(color_for(&bssid), "●");
        ui.label(format!("{} ({best} dBm)", aliases.name_for(&bssid, ssid)));
        if ui.small_button("◎").on_hover_text("Map this network only").clicked() {
            s.set_per_ap(Some(bssid));
        }
        if ui.small_button("✎").on_hover_text("Set alias").clicked() {
            s.alias_edit = Some((bssid, aliases.get(&bssid).unwrap_or_default().to_owned()));
        }
    });
}
