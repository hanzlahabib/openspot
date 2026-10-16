//! "Export report…" wizard: main, zones & snapshots, visualizations, view options, titles.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align2, Vec2};
use openspot_heatmap::{GradientPreset, Smoothing};
use openspot_model::{Project, Uuid};
use openspot_report::{ReportBranding, ReportKind, ReportOptions, Sections, Titles, ViewOptions, Visualization, sanitize_file_name};

use super::report_job::{self, Job, JobRequest};
use super::toolbar::{pick_file, report};
use crate::aliases::Aliases;
use crate::survey::Survey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Main,
    Zones,
    Visualizations,
    View,
    Titles,
}

impl Page {
    fn label(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Zones => "Zones & snapshots",
            Self::Visualizations => "Visualizations",
            Self::View => "View options",
            Self::Titles => "Titles & branding",
        }
    }
}

/// Wizard state; created when the window opens, dropped when it closes.
#[derive(Debug)]
struct Wizard {
    page: Page,
    kind: ReportKind,
    zones: BTreeSet<Uuid>,
    snapshots: BTreeSet<Uuid>,
    visualizations: BTreeSet<Visualization>,
    view: ViewOptions,
    titles: Titles,
    sections: Sections,
    output: Option<PathBuf>,
    open_after: bool,
    per_ap: bool,
    per_ap_dir: Option<PathBuf>,
}

/// Report-related state held by the survey: the saved branding and the open wizard / job.
#[derive(Debug, Default)]
pub struct ReportUi {
    /// Persisted in the preferences; edited on the wizard's "Titles & branding" page.
    pub branding: ReportBranding,
    wizard: Option<Wizard>,
    job: Option<Job>,
}

fn now_unix() -> i64 {
    // A clock before 1970 is broken; 0 still yields a valid (if odd-looking) report date.
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Wizard {
    fn new(s: &Survey) -> Self {
        Self {
            page: Page::Main,
            kind: ReportKind::Quick,
            zones: s.project.zones.iter().map(|z| z.id).collect(),
            snapshots: s.project.zones.iter().flat_map(|z| z.snapshots.iter().map(|x| x.id)).collect(),
            visualizations: BTreeSet::from([Visualization::SignalLevel]),
            // Start from what the user currently sees on screen.
            view: ViewOptions {
                show_points: s.show_points,
                show_ap_markers: s.show_ap_markers,
                gradient: s.gradient,
                smoothing: s.smoothing,
                opacity: s.opacity,
                ..ViewOptions::default()
            },
            titles: Titles::default(),
            sections: Sections::default(),
            output: None,
            open_after: true,
            per_ap: false,
            per_ap_dir: None,
        }
    }

    /// Selections that cover everything become `None` ("all"), so snapshots added later
    /// by a reused set of options would still be included.
    fn options(&self, project: &Project, generated_at: i64) -> ReportOptions {
        let all_zones: BTreeSet<Uuid> = project.zones.iter().map(|z| z.id).collect();
        let all_snaps: BTreeSet<Uuid> = project.zones.iter().flat_map(|z| z.snapshots.iter().map(|s| s.id)).collect();
        ReportOptions {
            kind: self.kind,
            zones: (self.zones != all_zones).then(|| self.zones.clone()),
            snapshots: (self.snapshots != all_snaps).then(|| self.snapshots.clone()),
            visualizations: self.visualizations.iter().copied().collect(),
            view: self.view,
            titles: self.titles.clone(),
            sections: self.sections,
            ..ReportOptions::advanced(generated_at)
        }
    }

    fn pages(&self) -> &'static [Page] {
        match self.kind {
            ReportKind::Quick => &[Page::Main, Page::Titles],
            ReportKind::Advanced => &[Page::Main, Page::Zones, Page::Visualizations, Page::View, Page::Titles],
        }
    }
}

fn default_pdf_name(project: &Project) -> String {
    format!("{}.pdf", sanitize_file_name(&project.name))
}

/// Toolbar entries: the report wizard and project archive import/export.
pub(super) fn toolbar_buttons(s: &mut Survey, ui: &mut egui::Ui) {
    if ui.add_enabled(s.report.job.is_none(), egui::Button::new("Export report…")).clicked() {
        s.report.wizard = Some(Wizard::new(s));
    }
    ui.menu_button("Archive ▾", |ui| {
        if ui.button("Export project archive…").on_hover_text("One .ospotz file with the whole project, for sharing").clicked() {
            export_archive(s);
            ui.close();
        }
        if ui.button("Import project archive…").clicked() {
            import_archive(s);
            ui.close();
        }
    });
}

fn export_archive(s: &mut Survey) {
    let name = format!("{}.{}", sanitize_file_name(&s.project.name), openspot_report::ARCHIVE_EXTENSION);
    let Some(path) = rfd::FileDialog::new().add_filter("OpenSpot archive", &[openspot_report::ARCHIVE_EXTENSION]).set_file_name(name).save_file() else {
        return;
    };
    let result = openspot_report::export_project(&s.project, &path, env!("CARGO_PKG_VERSION"), now_unix()).map(|_| ()).map_err(|e| e.to_string());
    report(s, result, "Project archive exported");
}

fn import_archive(s: &mut Survey) {
    let Some(archive) = pick_file("OpenSpot archive", &[openspot_report::ARCHIVE_EXTENSION]) else { return };
    let default = archive.file_stem().map_or_else(|| "imported".to_owned(), |n| n.to_string_lossy().into_owned());
    let Some(target) = rfd::FileDialog::new().add_filter("OpenSpot project", &["ospot"]).set_file_name(format!("{default}.ospot")).save_file() else {
        return;
    };
    let result = openspot_report::import_project(&archive, &target)
        .map_err(|e| format!("could not import {}: {e}", archive.display()))
        .and_then(|_| s.open_from(&target));
    report(s, result, "Project archive imported and opened");
}

pub(super) fn show(s: &mut Survey, ctx: &egui::Context, aliases: &Aliases) {
    poll_job(s, ctx);
    let Some(mut wizard) = s.report.wizard.take() else { return };
    let mut open = true;
    let (mut start, mut cancel_job, mut close) = (false, false, false);
    let running = s.report.job.is_some();
    egui::Window::new("Export report").open(&mut open).collapsible(false).resizable(true).default_width(540.0).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        ui.add_enabled_ui(!running, |ui| {
            ui.horizontal_wrapped(|ui| {
                for &page in wizard.pages() {
                    ui.selectable_value(&mut wizard.page, page, page.label());
                }
            });
            if !wizard.pages().contains(&wizard.page) {
                wizard.page = Page::Main;
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| match wizard.page {
                Page::Main => main_page(&mut wizard, &s.project, ui),
                Page::Zones => zones_page(&mut wizard, &s.project, ui),
                Page::Visualizations => visualizations_page(&mut wizard, ui),
                Page::View => view_page(&mut wizard, ui),
                Page::Titles => titles_page(&mut wizard, &mut s.report.branding, ui),
            });
        });
        ui.separator();
        if let Some(job) = &s.report.job {
            let p = &job.progress;
            let fraction = if p.total == 0 { 0.0 } else { p.done as f32 / p.total as f32 };
            ui.add(egui::ProgressBar::new(fraction).show_percentage().text(format!("{}: {}", p.phase, p.step)));
            cancel_job = ui.button("Cancel export").clicked();
        } else {
            ui.horizontal(|ui| {
                start = ui.button("Export").clicked();
                close = ui.button("Close").clicked();
            });
        }
    });
    if cancel_job && let Some(job) = &s.report.job {
        job.cancel();
    }
    if start {
        start_job(s, &mut wizard, aliases, ctx);
    }
    // The window stays open while a job runs so progress and cancel remain reachable.
    if (open && !close) || s.report.job.is_some() {
        s.report.wizard = Some(wizard);
    }
}

fn start_job(s: &mut Survey, w: &mut Wizard, aliases: &Aliases, ctx: &egui::Context) {
    if w.output.is_none() {
        w.output = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(default_pdf_name(&s.project)).save_file();
    }
    let Some(pdf_path) = w.output.clone() else { return };
    let per_ap_dir = if w.per_ap {
        let default = pdf_path.parent().map(|p| p.join(format!("{} heatmaps", sanitize_file_name(&s.project.name))));
        w.per_ap_dir.clone().or(default)
    } else {
        None
    };
    let request = JobRequest {
        aliases: report_job::alias_map(&s.project, aliases),
        project: s.project.clone(),
        branding: s.report.branding.clone(),
        options: w.options(&s.project, now_unix()),
        pdf_path,
        per_ap_dir,
    };
    let repaint = ctx.clone();
    s.report.job = Some(report_job::spawn(request, move || repaint.request_repaint()));
}

fn poll_job(s: &mut Survey, ctx: &egui::Context) {
    let Some(job) = s.report.job.as_mut() else { return };
    let Some(result) = job.poll() else {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        return;
    };
    s.report.job = None;
    let open_after = s.report.wizard.as_ref().is_some_and(|w| w.open_after);
    match result {
        Ok(out) => {
            s.error = None;
            let mut msg = format!("Report saved to {}", out.pdf.display());
            if out.heatmaps_written > 0 {
                msg.push_str(&format!(" and {} AP heatmaps", out.heatmaps_written));
            }
            if !out.skipped_zones.is_empty() {
                msg.push_str(&format!(" (no floor plan, heatmaps skipped: {})", out.skipped_zones.join(", ")));
            }
            s.message = Some(msg);
            s.report.wizard = None;
            if open_after && let Err(e) = report_job::open_with_default_app(&out.pdf) {
                s.error = Some(format!("report saved, but it could not be opened: {e}"));
            }
        }
        Err(e) => s.error = Some(e),
    }
}

fn path_row(ui: &mut egui::Ui, label: &str, path: &mut Option<PathBuf>, pick: impl FnOnce() -> Option<PathBuf>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.label(path.as_ref().map_or_else(|| "(not chosen yet)".to_owned(), |p| p.display().to_string()));
        if ui.button("Choose…").clicked() && let Some(p) = pick() {
            *path = Some(p);
        }
    });
}

fn main_page(w: &mut Wizard, project: &Project, ui: &mut egui::Ui) {
    ui.label("Report type");
    ui.radio_value(&mut w.kind, ReportKind::Quick, "Quick: every zone and snapshot, signal level map");
    ui.radio_value(&mut w.kind, ReportKind::Advanced, "Advanced: choose zones, snapshots, visualizations and options");
    ui.add_space(8.0);
    let name = default_pdf_name(project);
    path_row(ui, "File:", &mut w.output, || rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(name).save_file());
    ui.checkbox(&mut w.open_after, "Open the report after export");
    ui.checkbox(&mut w.per_ap, "Also export one heatmap image per access point");
    if w.per_ap {
        path_row(ui, "Folder:", &mut w.per_ap_dir, || rfd::FileDialog::new().pick_folder());
        if w.per_ap_dir.is_none() {
            ui.weak("Default: a folder next to the report.");
        }
    }
}

fn zones_page(w: &mut Wizard, project: &Project, ui: &mut egui::Ui) {
    ui.label("Choose what the report covers.");
    for zone in &project.zones {
        let mut zone_on = w.zones.contains(&zone.id);
        if ui.checkbox(&mut zone_on, &zone.name).changed() {
            for snap in &zone.snapshots {
                set(&mut w.snapshots, snap.id, zone_on);
            }
        }
        ui.indent(zone.id, |ui| {
            for snap in &zone.snapshots {
                let mut on = w.snapshots.contains(&snap.id);
                if ui.checkbox(&mut on, format!("{} ({} points)", snap.name, snap.points.len())).changed() {
                    set(&mut w.snapshots, snap.id, on);
                }
            }
        });
        // A zone counts as included while any of its snapshots is.
        set(&mut w.zones, zone.id, zone.snapshots.iter().any(|s| w.snapshots.contains(&s.id)));
    }
}

fn set<T: Ord>(set: &mut BTreeSet<T>, item: T, on: bool) {
    if on {
        set.insert(item);
    } else {
        set.remove(&item);
    }
}

fn visualizations_page(w: &mut Wizard, ui: &mut egui::Ui) {
    ui.label("One page per zone and snapshot is added for each selected map. Selecting none gives a report with cover and summary only.");
    for viz in Visualization::ALL {
        let mut on = w.visualizations.contains(&viz);
        if ui.checkbox(&mut on, format!("{}. {}", viz.number(), viz.title())).changed() {
            set(&mut w.visualizations, viz, on);
        }
    }
}

fn view_page(w: &mut Wizard, ui: &mut egui::Ui) {
    ui.checkbox(&mut w.view.show_plan, "Show floor plan");
    ui.checkbox(&mut w.view.show_path, "Show measurement path");
    ui.checkbox(&mut w.view.show_points, "Show measurement points");
    ui.checkbox(&mut w.view.show_ap_markers, "Show access point positions");
    egui::ComboBox::from_label("Colors").selected_text(w.view.gradient.label()).show_ui(ui, |ui| {
        for preset in GradientPreset::ALL {
            ui.selectable_value(&mut w.view.gradient, preset, preset.label());
        }
    });
    egui::ComboBox::from_label("Smoothing").selected_text(w.view.smoothing.label()).show_ui(ui, |ui| {
        for level in Smoothing::ALL {
            ui.selectable_value(&mut w.view.smoothing, level, level.label());
        }
    });
    ui.add(egui::Slider::new(&mut w.view.opacity, crate::survey::MIN_OPACITY..=255).text("heatmap opacity"));
    ui.separator();
    ui.label("Sections");
    ui.checkbox(&mut w.sections.cover, "Cover page");
    ui.checkbox(&mut w.sections.summary, "Summary tables");
    ui.checkbox(&mut w.sections.ap_tables, "Access point table on each heatmap page");
}

fn titles_page(w: &mut Wizard, brand: &mut ReportBranding, ui: &mut egui::Ui) {
    egui::Grid::new("titles").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        let line = |ui: &mut egui::Ui, label: &str, value: &mut String| {
            ui.label(label);
            ui.add(egui::TextEdit::singleline(value).desired_width(340.0));
            ui.end_row();
        };
        line(ui, "Title", &mut w.titles.title);
        line(ui, "Subtitle", &mut w.titles.subtitle);
        line(ui, "Company", &mut brand.company);
        line(ui, "Client", &mut brand.client);
        line(ui, "Location", &mut brand.location);
        line(ui, "Contact", &mut brand.contact);
        ui.label("Logo");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut brand.logo_path).desired_width(250.0).hint_text("PNG or JPEG"));
            if ui.button("Choose…").clicked() && let Some(p) = pick_file("Logo", &["png", "jpg", "jpeg"]) {
                brand.logo_path = p.to_string_lossy().into_owned();
            }
        });
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label("Notes (printed on the summary page)");
    ui.add(egui::TextEdit::multiline(&mut w.titles.notes).desired_rows(4).desired_width(f32::INFINITY));
    ui.weak("Company, client, location, contact and logo are remembered for next time.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::Snapshot;

    fn survey() -> Survey {
        let mut s = Survey::default();
        let mut z2 = openspot_model::Zone::new("Upper");
        z2.snapshots.push(Snapshot::new("Second"));
        s.project.zones.push(z2);
        s
    }

    #[test]
    fn full_selection_means_all_and_subsets_are_explicit() {
        let s = survey();
        let mut w = Wizard::new(&s);
        let o = w.options(&s.project, 7);
        assert!(o.zones.is_none() && o.snapshots.is_none());
        assert_eq!(o.generated_at, 7);
        assert_eq!(o.visualizations, vec![Visualization::SignalLevel]);

        let drop = s.project.zones[1].snapshots[1].id;
        w.snapshots.remove(&drop);
        let o = w.options(&s.project, 7);
        assert!(o.zones.is_none(), "zone selection unchanged");
        let snaps = o.snapshots.unwrap();
        assert_eq!(snaps.len(), 2);
        assert!(!snaps.contains(&drop));

        w.zones.remove(&s.project.zones[0].id);
        assert_eq!(w.options(&s.project, 7).zones.unwrap().len(), 1);
    }

    #[test]
    fn wizard_starts_from_the_current_view_and_kind_limits_pages() {
        let mut s = survey();
        s.show_points = false;
        s.opacity = 99;
        let mut w = Wizard::new(&s);
        assert!(!w.view.show_points && w.view.opacity == 99);
        assert!(w.view.show_plan, "plan always starts on");
        assert_eq!(w.pages(), &[Page::Main, Page::Titles]);
        w.kind = ReportKind::Advanced;
        assert_eq!(w.pages().len(), 5);
    }

    #[test]
    fn default_file_name_is_safe() {
        let mut p = Project::new("Site: A/B?");
        assert_eq!(default_pdf_name(&p), "Site_ A_B_.pdf");
        p.name = String::new();
        assert_eq!(default_pdf_name(&p), "unnamed.pdf");
    }

    #[test]
    fn new_project_keeps_branding() {
        let mut s = survey();
        s.report.branding.company = "Acme".into();
        s.new_project();
        assert_eq!(s.report.branding.company, "Acme");
    }
}
