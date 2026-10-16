#![forbid(unsafe_code)]
// No console window for the Windows GUI build.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod active_scan;
mod adapter_watch;
mod alias_csv;
mod aliases;
mod app;
mod columns;
mod csvio;
mod event_log;
mod grouping;
mod groups;
mod heat_ui;
mod inspector;
mod map_view;
mod open_arg;
mod planning;
mod link;
mod prefs;
mod survey;
mod sound;
mod spectrum;
mod state;
mod table_export;
mod theme;
mod update_check;
mod ui;
mod worker;

use eframe::egui;
use openspot_wlan::{FakeProvider, WlanProvider, native_provider};

fn main() -> eframe::Result {
    let use_fake = std::env::args().any(|a| a == "--fake");
    let startup = open_provider(use_fake);
    let initial_project = open_arg::project_arg(std::env::args().skip(1));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 760.0]).with_title("OpenSpot"),
        ..Default::default()
    };
    eframe::run_native(
        "OpenSpot",
        options,
        Box::new(move |cc| match startup {
            Ok((provider, adapters)) => Ok(Box::new(app::OpenSpotApp::new(&cc.egui_ctx, provider, adapters, initial_project.as_deref()))),
            Err(message) => Ok(Box::new(ErrorScreen(message))),
        }),
    )
}

type Startup = Result<(Box<dyn WlanProvider>, Vec<openspot_wlan::Adapter>), String>;

fn open_provider(use_fake: bool) -> Startup {
    let provider: Box<dyn WlanProvider> =
        if use_fake { Box::new(FakeProvider) } else { native_provider().map_err(|e| e.to_string())? };
    let adapters = provider.adapters().map_err(|e| e.to_string())?;
    Ok((provider, adapters))
}

/// Shown instead of the Inspector when no Wi-Fi backend/adapter is available.
struct ErrorScreen(String);

impl eframe::App for ErrorScreen {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Wi-Fi is not available");
            ui.label(&self.0);
            ui.label("Run with --fake to explore the UI with demo data.");
        });
    }
}
