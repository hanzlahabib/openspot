//! One page per (zone, snapshot, visualization): heatmap image, legend and AP table.

use std::collections::BTreeMap;

use image::{ImageFormat, Rgba, RgbaImage};
use openspot_export::render_png;
use openspot_heatmap::{Gradient, Metric, estimate_ap_position};
use openspot_model::{MapImage, Snapshot, Zone};
use openspot_wlan::MacAddress;

use super::builder::{ACCENT, Builder, CONTENT_TOP, CONTENT_W, Column, MARGIN, MUTED, RULE, TEXT};
use super::networks::{NetworkRow, network_rows};
use crate::error::ReportError;
use crate::options::{ReportOptions, ViewOptions, Visualization};
use crate::pdf::{PAGE_W, Weight};
use crate::timefmt::iso_date;

/// Longest side, in pixels, of a heatmap image embedded in the PDF. The on-screen raster
/// is only 160 cells wide and interpolated, so more pixels add file size, not detail.
const HEATMAP_MAX_SIDE_PX: u32 = 1800;
/// Tallest the heatmap may be on the page, leaving room for legend and table.
const IMAGE_MAX_H: f32 = 370.0;
/// Background used when the floor plan is hidden.
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
const POINT_RADIUS_PT: f32 = 2.3;
const MARKER_SIZE_PT: f32 = 5.0;
const GRADIENT_STEPS: usize = 64;
const GRADIENT_BAR: (f32, f32) = (260.0, 10.0);
const AP_COLUMNS: [Column; 8] = [
    Column::left("#", 24.0),
    Column::left("SSID / alias", 190.0),
    Column::left("BSSID", 88.0),
    Column::right("Ch", 26.0),
    Column::left("Band", 42.0),
    Column::right("Width", 46.0),
    Column::right("Points", 45.0),
    Column::right("Max dBm", 50.0),
];

#[derive(Debug, Clone, Copy)]
pub struct HeatmapPage<'a> {
    pub zone_index: usize,
    pub zone: &'a Zone,
    pub snapshot_index: usize,
    pub snapshot: &'a Snapshot,
    pub viz: Visualization,
}

impl HeatmapPage<'_> {
    /// `#zone.snapshot.visualization`, 1-based.
    pub fn number(&self) -> String {
        format!("#{}.{}.{}", self.zone_index + 1, self.snapshot_index + 1, self.viz.number())
    }
}

/// A map-less stand-in zone for the "hide floor plan" option: white, same size, so
/// measurement coordinates still line up.
fn blank_zone(map: &MapImage) -> Result<Zone, ReportError> {
    let mut png = std::io::Cursor::new(Vec::new());
    RgbaImage::from_pixel(map.width.max(1), map.height.max(1), WHITE).write_to(&mut png, ImageFormat::Png)?;
    let mut zone = Zone::new("blank");
    zone.snapshots.clear();
    zone.map = Some(MapImage { original_name: "blank.png".into(), data: png.into_inner(), width: map.width, height: map.height });
    Ok(zone)
}

/// The PNG of `metric` over the zone's plan (or a white page when the plan is hidden), or
/// `None` when the zone has no floor plan. `draw_points` bakes the measurement points
/// into the raster; reports leave it off and draw them as vector shapes instead.
pub(crate) fn render_heatmap(zone: &Zone, snapshot: &Snapshot, view: &ViewOptions, metric: &Metric, draw_points: bool) -> Result<Option<Vec<u8>>, ReportError> {
    let Some(map) = zone.map.as_ref() else { return Ok(None) };
    let blank;
    let shown = if view.show_plan {
        zone
    } else {
        blank = blank_zone(map)?;
        &blank
    };
    render_png(shown, snapshot, metric, view.gradient, view.smoothing, view.opacity, draw_points)
        .map(Some)
        .map_err(|source| ReportError::Heatmap { zone: zone.name.clone(), source })
}

pub fn heatmap_page(b: &mut Builder, page: HeatmapPage<'_>, opts: &ReportOptions, aliases: &BTreeMap<MacAddress, String>) -> Result<(), ReportError> {
    let metric = page.viz.metric();
    let png = render_heatmap(page.zone, page.snapshot, &opts.view, &metric, false)?;

    b.start_page();
    let mut y = CONTENT_TOP + 14.0;
    b.doc.text(MARGIN, y, 15.0, Weight::Bold, TEXT, page.viz.title());
    b.doc.text_right(PAGE_W - MARGIN, y, 15.0, Weight::Bold, ACCENT, &page.number());
    y += 15.0;
    let date = page.snapshot.points.iter().map(|p| p.time).filter(|&t| t > 0).max().map(iso_date);
    let mut sub = format!("Zone: {}   Snapshot: {}   Points: {}", page.zone.name, page.snapshot.name, page.snapshot.points.len());
    if let Some(d) = date {
        sub.push_str(&format!("   Measured: {d}"));
    }
    let sub = crate::pdf::truncate(b.doc.face(Weight::Regular), &b.doc.sanitize(&sub), 9.0, CONTENT_W);
    b.doc.text(MARGIN, y, 9.0, Weight::Regular, MUTED, &sub);
    y += 12.0;

    let rows = network_rows([page.snapshot], aliases);
    let shown = opts.max_ap_rows.map_or(rows.len(), |n| n.min(rows.len()));
    let listed = &rows[..shown];

    match png {
        Some(bytes) => {
            let (id, w, h) = b.doc.add_image(&bytes, HEATMAP_MAX_SIDE_PX)?;
            let k = (CONTENT_W / w as f32).min(IMAGE_MAX_H / h as f32);
            let (dw, dh) = (w as f32 * k, h as f32 * k);
            let x0 = MARGIN + (CONTENT_W - dw) / 2.0;
            b.doc.image(id, x0, y, dw, dh);
            overlay(b, &page, opts, listed, (x0, y, k));
            b.doc.stroke_rect(x0, y, dw, dh, 0.5, RULE);
            y += dh + 8.0;
        }
        None => {
            b.doc.stroke_rect(MARGIN, y, CONTENT_W, 90.0, 0.5, RULE);
            b.doc.text_centered(PAGE_W / 2.0, y + 48.0, 10.0, Weight::Regular, MUTED, "This zone has no floor plan, so no heatmap can be drawn.");
            y += 98.0;
        }
    }
    if page.snapshot.points.is_empty() {
        b.doc.text(MARGIN, y + 8.0, 9.0, Weight::Regular, MUTED, "This snapshot has no measurement points.");
        y += 16.0;
    }
    b.y = y;
    legend(b, &metric, opts);

    if opts.sections.ap_tables {
        b.gap(6.0);
        b.heading("Access points", 11.0);
        b.gap(3.0);
        if rows.is_empty() {
            b.paragraph("No access points were measured in this snapshot.", 9.0, MUTED);
        } else {
            if shown < rows.len() {
                b.paragraph(&format!("Strongest {shown} of {} networks; all networks are listed in the summary.", rows.len()), 8.5, MUTED);
            }
            let markers = marker_numbers(page.snapshot, listed);
            let cells: Vec<Vec<String>> = listed
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    vec![
                        markers[i].map_or_else(|| "-".to_owned(), |m| m.to_string()),
                        n.display_name(),
                        n.bssid.to_string(),
                        if n.channel == 0 { "-".to_owned() } else { n.channel.to_string() },
                        n.band_label().to_owned(),
                        n.width_label(),
                        n.points_heard.to_string(),
                        n.max_rssi_dbm.to_string(),
                    ]
                })
                .collect();
            b.table(&AP_COLUMNS, &cells);
        }
    }
    Ok(())
}

/// Marker number (the row rank) per listed AP, `None` where the AP was heard too rarely
/// to estimate a position.
fn marker_numbers(snapshot: &Snapshot, listed: &[NetworkRow]) -> Vec<Option<usize>> {
    listed.iter().enumerate().map(|(i, n)| estimate_ap_position(snapshot, &n.bssid).map(|_| i + 1)).collect()
}

/// Vector overlay on the placed image: path, measurement points, AP markers.
/// `place` is (left, top, points per map pixel).
fn overlay(b: &mut Builder, page: &HeatmapPage<'_>, opts: &ReportOptions, listed: &[NetworkRow], place: (f32, f32, f32)) {
    let (x0, y0, k) = place;
    let at = |x: f64, y: f64| (x0 + x as f32 * k, y0 + y as f32 * k);
    let mut points: Vec<_> = page.snapshot.points.iter().collect();
    points.sort_by_key(|p| p.time);
    if opts.view.show_path {
        let path: Vec<_> = points.iter().map(|p| at(p.x, p.y)).collect();
        b.doc.polyline(&path, 0.8, [60, 60, 60]);
    }
    if opts.view.show_points {
        let gradient = Gradient::signal_dbm();
        for p in &points {
            let best = p.access_points.iter().map(|a| a.rssi_dbm).max().map_or(-100.0, |v| v as f32);
            let (cx, cy) = at(p.x, p.y);
            b.doc.circle(cx, cy, POINT_RADIUS_PT, gradient.color(best), Some((0.6, [255, 255, 255])));
        }
    }
    if opts.view.show_ap_markers {
        for (i, n) in listed.iter().enumerate() {
            let Some(est) = estimate_ap_position(page.snapshot, &n.bssid) else { continue };
            let (cx, cy) = at(est.x, est.y);
            let s = MARKER_SIZE_PT;
            b.doc.polygon(&[(cx, cy - s), (cx + s, cy + s * 0.8), (cx - s, cy + s * 0.8)], [20, 20, 20], Some((0.8, [255, 255, 255])));
            b.doc.text(cx + s + 1.5, cy + 3.0, 8.0, Weight::Bold, [20, 20, 20], &(i + 1).to_string());
        }
    }
}

/// Colour key under the heatmap: gradient bar for continuous metrics, labelled swatches
/// for categorical ones.
fn legend(b: &mut Builder, metric: &Metric, opts: &ReportOptions) {
    b.ensure(44.0);
    let top = b.y;
    let unit = metric.unit();
    b.doc.text(MARGIN, top + 9.0, 9.0, Weight::Bold, TEXT, if unit.is_empty() { "Legend".to_owned() } else { format!("Legend ({unit})") }.as_str());
    let y = top + 16.0;
    match metric.palette() {
        Some(palette) => {
            let mut x = MARGIN;
            let mut row_y = y;
            for (label, color) in palette.entries() {
                let w = 14.0 + b.doc.measure(Weight::Regular, 8.5, label) + 14.0;
                if x + w > MARGIN + CONTENT_W {
                    x = MARGIN;
                    row_y += 14.0;
                }
                b.doc.fill_rect(x, row_y, 9.0, 9.0, *color);
                b.doc.stroke_rect(x, row_y, 9.0, 9.0, 0.4, RULE);
                b.doc.text(x + 13.0, row_y + 8.0, 8.5, Weight::Regular, TEXT, label);
                x += w;
            }
            b.y = row_y + 14.0;
            if let Metric::Troubleshoot { base, acceptable, critical } = metric {
                let u = base.unit();
                let note = format!("Good: >= {acceptable} {u}   Critical: <= {critical} {u}   Acceptable in between.");
                b.doc.text(MARGIN, b.y + 6.0, 8.5, Weight::Regular, MUTED, &note);
                b.y += 12.0;
            }
        }
        None => {
            let gradient = metric.gradient().with_preset(opts.view.gradient);
            let (lo, hi) = gradient.range();
            let (bar_w, bar_h) = GRADIENT_BAR;
            let step = bar_w / GRADIENT_STEPS as f32;
            for i in 0..GRADIENT_STEPS {
                let v = lo + (hi - lo) * (i as f32 + 0.5) / GRADIENT_STEPS as f32;
                // Overdraw by a hair so anti-aliasing leaves no seams between steps.
                b.doc.fill_rect(MARGIN + i as f32 * step, y, step + 0.3, bar_h, gradient.color(v));
            }
            b.doc.stroke_rect(MARGIN, y, bar_w, bar_h, 0.4, RULE);
            const TICKS: usize = 4;
            for t in 0..=TICKS {
                let v = lo + (hi - lo) * t as f32 / TICKS as f32;
                let tx = MARGIN + bar_w * t as f32 / TICKS as f32;
                b.doc.text_centered(tx, y + bar_h + 10.0, 8.0, Weight::Regular, MUTED, &format!("{v:.0}"));
            }
            b.y = y + bar_h + 16.0;
        }
    }
}

