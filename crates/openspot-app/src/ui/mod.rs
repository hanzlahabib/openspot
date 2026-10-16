pub mod advisor;
pub mod alias;
pub mod alias_io;
pub mod charts;
pub mod dialog;
pub mod event_log;
pub mod groups;
pub mod presets;
pub mod prefs_window;
pub mod table;

use eframe::egui::{Color32, ecolor::Hsva};
use openspot_wlan::MacAddress;

/// Stable per-AP chart color so a network keeps its color across frames and views.
pub fn color_for(bssid: &MacAddress) -> Color32 {
    // FNV-1a over the address bytes; spread hues with the golden-ratio conjugate.
    let hash = bssid.0.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let hue = (hash % 1000) as f32 / 1000.0;
    Hsva::new(hue, 0.65, 0.85, 1.0).into()
}
