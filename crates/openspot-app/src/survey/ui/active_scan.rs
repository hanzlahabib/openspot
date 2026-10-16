//! egui pieces of the active scan: toolbar toggle, settings window, metric entries, status.

use eframe::egui::{self, Color32};
use openspot_heatmap::SpeedKind;

use crate::active_scan::ConnectionTest;
use crate::survey::{MetricChoice, Survey};

/// Toggle plus settings button, placed in the survey toolbar.
pub(super) fn toolbar(s: &mut Survey, ui: &mut egui::Ui) {
    ui.checkbox(&mut s.active.prefs.enabled, "Active scan")
        .on_hover_text("After each passive scan, run the speed tests chosen in the settings");
    if ui.button("Active scan settings…").clicked() {
        s.active.settings_open = true;
    }
}

/// The speed and jitter maps, appended to the metric combo box.
pub(super) fn metric_entries(s: &mut Survey, ui: &mut egui::Ui) {
    ui.separator();
    for kind in SpeedKind::ALL {
        ui.selectable_value(&mut s.metric_choice, MetricChoice::Speed(kind), kind.label());
    }
}

/// Progress text and a Cancel button while a point is being measured.
pub(super) fn status(s: &Survey, ui: &mut egui::Ui) {
    if let Some(progress) = &s.active.progress {
        ui.label(format!("· {progress}"));
        if ui.small_button("Cancel").on_hover_text("Stop the speed tests; the passive scan is kept").clicked() {
            s.active.cancel.cancel();
        }
    }
}

fn number<T: egui::emath::Numeric>(ui: &mut egui::Ui, label: &str, value: &mut T, range: std::ops::RangeInclusive<T>) {
    ui.label(label);
    ui.add(egui::DragValue::new(value).range(range));
    ui.end_row();
}

fn text(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(320.0));
    ui.end_row();
}

pub(super) fn settings_window(s: &mut Survey, ctx: &egui::Context) {
    s.active.poll_test();
    if !s.active.settings_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Active scan settings").open(&mut open).resizable(false).show(ctx, |ui| {
        let a = &mut s.active;
        ui.label("Runs after each passive scan. Nothing is sent anywhere until you enter a server below.");
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(&mut a.prefs.use_http, "HTTP");
            ui.checkbox(&mut a.prefs.use_iperf_tcp, "iperf3 TCP");
            ui.checkbox(&mut a.prefs.use_iperf_udp, "iperf3 UDP");
        });
        egui::Grid::new("active_scan_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            if a.prefs.use_http {
                text(ui, "Download URL", &mut a.prefs.http_download_url, "https://…");
                number(ui, "Download size (MB)", &mut a.prefs.http_download_mb, 1..=1024);
                text(ui, "Upload URL", &mut a.prefs.http_upload_url, "https://… (accepts POST)");
                number(ui, "Upload size (MB)", &mut a.prefs.http_upload_mb, 1..=1024);
                number(ui, "HTTP timeout (s)", &mut a.prefs.http_timeout_secs, 5..=600);
            }
            if a.prefs.use_iperf_tcp || a.prefs.use_iperf_udp {
                text(ui, "iperf3 server", &mut a.prefs.iperf_host, "host or IP of a running `iperf3 -s`");
                number(ui, "Port", &mut a.prefs.iperf_port, 1..=65535);
                text(ui, "iperf3 program", &mut a.prefs.iperf_binary, "empty = find `iperf3` on PATH");
                number(ui, "Duration (s)", &mut a.prefs.duration_secs, 1..=600);
                number(ui, "Parallel streams", &mut a.prefs.streams, 1..=128);
                number(ui, "Window (KiB, 0 = default)", &mut a.prefs.window_kib, 0..=65536);
            }
            if a.prefs.use_iperf_udp {
                number(ui, "UDP rate (Mbit/s, 0 = unlimited)", &mut a.prefs.udp_bitrate_mbps, 0.0..=10_000.0);
                ui.label("");
                ui.checkbox(&mut a.prefs.udp_reuse_tcp, "Send UDP at the TCP result when available");
                ui.end_row();
            }
            number(ui, "Delay before testing (s)", &mut a.prefs.delay_secs, 0..=120);
        });
        ui.separator();
        ui.label("Networks to test (saved Wi-Fi profile names, one per line). Empty = current connection only.");
        if ui.add(egui::TextEdit::multiline(&mut a.ssids_text).desired_rows(3).desired_width(380.0)).changed() {
            a.prefs.ssids = a.ssids_text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect();
        }
        ui.small("Switching networks needs saved profiles and is Windows-only; your original network is restored afterwards.");
        ui.separator();
        let running = matches!(a.test, ConnectionTest::Running(_));
        if ui.add_enabled(!running, egui::Button::new("Test connection")).clicked() {
            let repaint = ctx.clone();
            a.start_test(move || repaint.request_repaint());
        }
        match &a.test {
            ConnectionTest::Idle => {}
            ConnectionTest::Running(_) => {
                ui.spinner();
            }
            ConnectionTest::Done(lines) => {
                for line in lines {
                    let failed = line.contains("FAILED") || !line.contains(" OK") && !line.contains("found") && !line.contains("reachable");
                    ui.colored_label(if failed { Color32::LIGHT_RED } else { Color32::LIGHT_GREEN }, line);
                }
            }
        }
    });
    s.active.settings_open = open;
    if !open {
        // Keep stored values valid even if a field was left half-edited.
        s.active.prefs = std::mem::take(&mut s.active.prefs).sanitized();
    }
}
