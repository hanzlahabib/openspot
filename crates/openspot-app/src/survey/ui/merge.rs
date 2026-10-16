//! Survey UI: merge snapshots dialog.

use eframe::egui::{self, Align2, Vec2};

use crate::survey::Survey;

pub(super) fn dialog(s: &mut Survey, ctx: &egui::Context) {
    let Some(mut chosen) = s.merge_dialog.clone() else { return };
    let names: Vec<String> = s.zone().snapshots.iter().map(|x| x.name.clone()).collect();
    let (mut merge, mut cancel) = (false, false);
    egui::Window::new("Merge snapshots").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.label("Choose two or more snapshots. A new snapshot is created; the originals stay untouched.");
        for (i, name) in names.iter().enumerate() {
            let mut on = chosen.contains(&i);
            if ui.checkbox(&mut on, name).changed() {
                if on {
                    chosen.insert(i);
                } else {
                    chosen.remove(&i);
                }
            }
        }
        let provenance = s.snapshot_provenance();
        if !provenance.is_empty() {
            ui.separator();
            ui.label(format!("The current snapshot “{}” was merged from:", names[s.snapshot]));
            for p in provenance {
                ui.label(format!("  {} · kept {} of {} points", p.source_name, p.points_kept, p.points_in));
            }
            ui.separator();
        }
        ui.label(format!("Points closer than {:.1} px taken within the same hour count as duplicates.", s.merge_epsilon_px()));
        ui.horizontal(|ui| {
            merge = ui.add_enabled(chosen.len() >= 2, egui::Button::new("Merge")).clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if merge {
        let indices: Vec<usize> = chosen.iter().copied().collect();
        match s.merge_selected_snapshots(&indices) {
            Ok(_) => s.error = None,
            Err(e) => s.error = Some(e.to_string()),
        }
        s.merge_dialog = None;
    } else if cancel {
        s.merge_dialog = None;
    } else {
        s.merge_dialog = Some(chosen);
    }
}
