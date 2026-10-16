use eframe::egui;
use openspot_wlan::{AccessPoint, Band, recommend};

/// Best channels shown per band; more would just be noise.
const TOP_N: usize = 4;

/// Channel advisor: least-congested channels per band for the chosen width.
pub fn show(ui: &mut egui::Ui, aps: &[AccessPoint], width_mhz: &mut u16) {
    ui.horizontal(|ui| {
        ui.label("Channel width:");
        for w in [20u16, 40, 80, 160] {
            ui.selectable_value(width_mhz, w, format!("{w} MHz"));
        }
        ui.weak("Based on every network currently heard, weighted by signal strength and spectrum overlap.");
    });
    ui.separator();
    ui.columns(3, |cols| {
        for (col, band) in cols.iter_mut().zip([Band::Ghz2_4, Band::Ghz5, Band::Ghz6]) {
            col.heading(band.label());
            let scores = recommend(aps, band, *width_mhz);
            if scores.is_empty() {
                col.label(format!("No {width_mhz} MHz channels recommended in this band."));
                continue;
            }
            let heard = aps.iter().filter(|a| a.band == Some(band)).count();
            col.weak(format!("{heard} networks heard"));
            for (i, s) in scores.iter().take(TOP_N).enumerate() {
                let dfs = if s.dfs { " · DFS" } else { "" };
                let text = format!(
                    "{}. channel {} · {} · {} network{} overlap{dfs}",
                    i + 1,
                    s.channel,
                    s.congestion(),
                    s.overlapping_aps,
                    if s.overlapping_aps == 1 { "" } else { "s" },
                );
                if i == 0 {
                    col.strong(text);
                } else {
                    col.label(text);
                }
            }
        }
    });
}
