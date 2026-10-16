//! Cover page and summary tables.

use std::collections::BTreeMap;

use openspot_wlan::MacAddress;

use super::builder::{ACCENT, Builder, CONTENT_W, Column, MARGIN, MUTED, RULE, TEXT};
use super::networks::{NetworkRow, network_rows};
use super::Plan;
use crate::branding::ReportBranding;
use crate::error::ReportError;
use crate::options::{ReportKind, ReportOptions};
use crate::pdf::{PAGE_W, Weight, wrap};
use crate::timefmt::iso_date;

/// Longest side, in pixels, of a logo embedded in the PDF: sharp at print size on the
/// cover, small enough not to bloat the file.
const LOGO_MAX_SIDE_PX: u32 = 800;
const LOGO_BOX_W: f32 = 170.0;
const LOGO_BOX_H: f32 = 70.0;
pub const DEFAULT_TITLE: &str = "Wi-Fi Site Survey Report";

/// First and last measurement day of the planned snapshots, formatted for display.
pub fn survey_dates(plan: &Plan<'_>) -> Option<String> {
    // Time 0 means "not recorded"; including it would claim a survey in 1970.
    let times = plan.zones.iter().flat_map(|z| &z.snapshots).flat_map(|(_, s)| &s.points).map(|p| p.time).filter(|&t| t > 0);
    let (lo, hi) = times.fold((i64::MAX, i64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
    if lo > hi {
        return None;
    }
    let (a, b) = (iso_date(lo), iso_date(hi));
    Some(if a == b { a } else { format!("{a} to {b}") })
}

pub fn cover(
    b: &mut Builder,
    plan: &Plan<'_>,
    project_name: &str,
    brand: &ReportBranding,
    logo: Option<&[u8]>,
    opts: &ReportOptions,
) -> Result<(), ReportError> {
    b.start_page_bare();
    let mut y = 56.0;
    if let Some(bytes) = logo {
        let (id, w, h) = b.doc.add_image(bytes, LOGO_MAX_SIDE_PX).map_err(|e| match e {
            ReportError::Image(inner) => ReportError::Logo(inner),
            other => other,
        })?;
        let k = (LOGO_BOX_W / w as f32).min(LOGO_BOX_H / h as f32);
        b.doc.image(id, MARGIN, y, w as f32 * k, h as f32 * k);
        y += h as f32 * k + 12.0;
    }
    if !brand.company.trim().is_empty() {
        b.doc.text(MARGIN, y + 12.0, 13.0, Weight::Bold, TEXT, brand.company.trim());
    }

    let title = if opts.titles.title.trim().is_empty() { DEFAULT_TITLE } else { opts.titles.title.trim() };
    let mut ty = 300.0;
    b.doc.fill_rect(MARGIN, ty - 34.0, 56.0, 4.0, ACCENT);
    let clean = b.doc.sanitize(title);
    for line in wrap(b.doc.face(Weight::Bold), &clean, 26.0, CONTENT_W) {
        b.doc.text(MARGIN, ty, 26.0, Weight::Bold, TEXT, &line);
        ty += 32.0;
    }
    let sub = if opts.titles.subtitle.trim().is_empty() {
        match opts.kind {
            ReportKind::Quick => "Quick report",
            ReportKind::Advanced => "Advanced report",
        }
    } else {
        opts.titles.subtitle.trim()
    };
    let clean = b.doc.sanitize(sub);
    for line in wrap(b.doc.face(Weight::Regular), &clean, 14.0, CONTENT_W) {
        ty += 4.0;
        b.doc.text(MARGIN, ty, 14.0, Weight::Regular, MUTED, &line);
        ty += 14.0;
    }

    let dates = survey_dates(plan).unwrap_or_else(|| "No measurements".to_owned());
    let details: [(&str, String); 7] = [
        ("Project", project_name.to_owned()),
        ("Client", brand.client.trim().to_owned()),
        ("Location", brand.location.trim().to_owned()),
        ("Prepared by", brand.company.trim().to_owned()),
        ("Contact", brand.contact.trim().to_owned()),
        ("Survey dates", dates),
        ("Report date", iso_date(opts.generated_at)),
    ];
    let mut dy = ty.max(300.0) + 70.0;
    b.doc.line(MARGIN, dy - 18.0, PAGE_W - MARGIN, dy - 18.0, 0.6, RULE);
    const LABEL_W: f32 = 100.0;
    for (label, value) in details.iter().filter(|(_, v)| !v.is_empty()) {
        b.doc.text(MARGIN, dy, 10.0, Weight::Bold, MUTED, label);
        let clean = b.doc.sanitize(value);
        for line in wrap(b.doc.face(Weight::Regular), &clean, 10.5, CONTENT_W - LABEL_W) {
            b.doc.text(MARGIN + LABEL_W, dy, 10.5, Weight::Regular, TEXT, &line);
            dy += 14.0;
        }
        dy += 5.0;
    }
    Ok(())
}

pub fn summary(
    b: &mut Builder,
    plan: &Plan<'_>,
    aliases: &BTreeMap<MacAddress, String>,
    opts: &ReportOptions,
    visualization_count: usize,
) {
    b.start_page();
    b.heading("Summary", 18.0);
    b.gap(6.0);
    let snapshots: Vec<_> = plan.zones.iter().flat_map(|z| z.snapshots.iter().map(move |(_, s)| (z, *s))).collect();
    let points: usize = snapshots.iter().map(|(_, s)| s.points.len()).sum();
    let networks = network_rows(snapshots.iter().map(|(_, s)| *s), aliases);
    b.paragraph(
        &format!(
            "{} zone{}, {} snapshot{}, {} measurement point{}, {} network{}.",
            plan.zones.len(),
            plural(plan.zones.len()),
            snapshots.len(),
            plural(snapshots.len()),
            points,
            plural(points),
            networks.len(),
            plural(networks.len()),
        ),
        10.0,
        TEXT,
    );
    if !opts.titles.notes.trim().is_empty() {
        b.gap(4.0);
        b.paragraph(opts.titles.notes.trim(), 10.0, TEXT);
    }
    if visualization_count == 0 {
        b.paragraph("No heatmap visualizations were selected for this report.", 10.0, MUTED);
    }

    b.gap(8.0);
    b.heading("Zones and snapshots", 12.0);
    b.gap(4.0);
    if snapshots.is_empty() {
        b.paragraph("No zones or snapshots were selected.", 9.5, MUTED);
    } else {
        const COLS: [Column; 6] = [
            Column::left("Zone", 120.0),
            Column::left("Snapshot", 110.0),
            Column::right("Points", 50.0),
            Column::right("Networks", 58.0),
            Column::left("Survey date", 110.0),
            Column::right("Scale", 63.0),
        ];
        let rows: Vec<Vec<String>> = snapshots
            .iter()
            .map(|(z, s)| {
                let date = s.points.iter().map(|p| p.time).filter(|&t| t > 0).max().map_or_else(|| "-".to_owned(), iso_date);
                let scale = z.zone.scale.map_or_else(|| "not set".to_owned(), |sc| format!("{:.1} px/m", sc.pixels_per_meter()));
                vec![
                    z.zone.name.clone(),
                    s.name.clone(),
                    s.points.len().to_string(),
                    network_rows([*s], aliases).len().to_string(),
                    date,
                    scale,
                ]
            })
            .collect();
        b.table(&COLS, &rows);
    }

    b.gap(10.0);
    b.heading("Networks", 12.0);
    b.gap(4.0);
    if networks.is_empty() {
        b.paragraph("No networks were measured.", 9.5, MUTED);
    } else {
        b.table(&NETWORK_COLUMNS, &network_cells(&networks));
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

const NETWORK_COLUMNS: [Column; 8] = [
    Column::left("SSID / alias", 108.0),
    Column::left("BSSID", 82.0),
    Column::right("Ch", 24.0),
    Column::left("Band", 40.0),
    Column::right("Width", 44.0),
    Column::left("Security", 82.0),
    Column::left("Vendor", 83.0),
    Column::right("Max dBm", 48.0),
];

fn network_cells(rows: &[NetworkRow]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|n| {
            vec![
                n.display_name(),
                n.bssid.to_string(),
                if n.channel == 0 { "-".to_owned() } else { n.channel.to_string() },
                n.band_label().to_owned(),
                n.width_label(),
                n.security.clone(),
                n.vendor.unwrap_or("-").to_owned(),
                n.max_rssi_dbm.to_string(),
            ]
        })
        .collect()
}
