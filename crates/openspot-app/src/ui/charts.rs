use std::collections::BTreeSet;

use eframe::egui::{self, Stroke};
use egui_plot::{Legend, Line, Plot, PlotPoints, Polygon};
use openspot_wlan::{Band, MacAddress};

use super::color_for;
use crate::aliases::Aliases;
use crate::spectrum;
use crate::state::{Track, Tracker};

/// Bottom of the signal axis; anything weaker is not usable and clutters the chart.
const FLOOR_DBM: f64 = -100.0;
const CEILING_DBM: f64 = -20.0;

/// RSSI of the selected APs over the last `window_secs` seconds (x = seconds ago).
pub fn rssi_over_time(
    ui: &mut egui::Ui,
    tracker: &Tracker,
    selected: &BTreeSet<MacAddress>,
    aliases: &Aliases,
    now: f64,
    window_secs: f64,
) {
    if selected.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label("Click networks in the table to compare their signal over time.");
        });
        return;
    }
    Plot::new("rssi_time")
        .legend(Legend::default())
        .include_y(FLOOR_DBM)
        .include_y(CEILING_DBM)
        .include_x(-window_secs)
        .include_x(0.0)
        .x_axis_label("seconds ago")
        .y_axis_label("dBm")
        .allow_scroll(false)
        .show(ui, |plot| {
            for bssid in selected {
                let Some(track) = tracker.get(bssid) else { continue };
                let points: Vec<[f64; 2]> = track
                    .samples
                    .iter()
                    .filter(|s| now - s.t <= window_secs)
                    .map(|s| [s.t - now, f64::from(s.rssi_dbm)])
                    .collect();
                plot.line(
                    Line::new(aliases.name_for(bssid, &track.ap.ssid), PlotPoints::new(points))
                        .color(color_for(bssid))
                        .width(2.0),
                );
            }
        });
}

/// Every visible AP of one band drawn at its frequency span.
pub fn spectrum(ui: &mut egui::Ui, rows: &[&Track], band: Band, selected: &BTreeSet<MacAddress>, aliases: &Aliases) {
    let (lo, hi) = spectrum::band_range_mhz(band);
    Plot::new(("spectrum", band.label()))
        .legend(Legend::default())
        .include_x(lo)
        .include_x(hi)
        .include_y(FLOOR_DBM)
        .include_y(CEILING_DBM)
        .x_axis_label("MHz")
        .y_axis_label("dBm")
        .allow_scroll(false)
        .show(ui, |plot| {
            for track in rows.iter().filter(|t| t.ap.band == Some(band)) {
                let color = color_for(&track.ap.bssid);
                let emphasized = selected.contains(&track.ap.bssid);
                let fill = color.gamma_multiply(if emphasized { 0.45 } else { 0.18 });
                let stroke = Stroke::new(if emphasized { 2.5 } else { 1.2 }, color);
                plot.polygon(
                    Polygon::new(
                        aliases.name_for(&track.ap.bssid, &track.ap.ssid),
                        PlotPoints::new(spectrum::shape(&track.ap, FLOOR_DBM)),
                    )
                    .fill_color(fill)
                    .stroke(stroke),
                );
            }
        });
}
