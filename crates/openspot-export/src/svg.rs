//! Scalable (SVG) export of heatmap isolines.

use std::fmt::Write as _;

use openspot_heatmap::{GradientPreset, Isoline, Metric, effective_gradient, evenly_spaced_levels, isolines, samples};
use openspot_model::{Snapshot, Zone};

use crate::ExportError;

/// Stroke width in SVG user units (map pixels): visible on a typical plan without
/// hiding the plan; `non-scaling-stroke` keeps it constant when zooming the file.
const STROKE_WIDTH: f64 = 2.0;

/// SVG document of `lines` over a `width_px` x `height_px` map. `color` picks a level's
/// stroke color and `label` its tooltip text.
pub fn isolines_to_svg(width_px: f64, height_px: f64, lines: &[Isoline], color: impl Fn(f32) -> [u8; 3], label: impl Fn(f32) -> String) -> String {
    let mut svg = String::new();
    // Writing to a String cannot fail; the results are discarded deliberately.
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width_px} {height_px}" width="{width_px}" height="{height_px}">"#
    );
    for iso in lines {
        let [r, g, b] = color(iso.level);
        let title = escape(&label(iso.level));
        for line in &iso.lines {
            // A closed polyline repeats its first point; `Z` closes it instead.
            let pts = if line.closed { &line.points[..line.points.len().saturating_sub(1)] } else { &line.points[..] };
            let Some((first, rest)) = pts.split_first() else { continue };
            let mut d = format!("M{:.2} {:.2}", first.0, first.1);
            for p in rest {
                let _ = write!(d, " L{:.2} {:.2}", p.0, p.1);
            }
            if line.closed {
                d.push_str(" Z");
            }
            let _ = writeln!(
                svg,
                r#"<path d="{d}" fill="none" stroke="rgb({r},{g},{b})" stroke-width="{STROKE_WIDTH}" stroke-linejoin="round" vector-effect="non-scaling-stroke"><title>{title}</title></path>"#
            );
        }
    }
    svg.push_str("</svg>\n");
    svg
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Isolines of a continuous `metric` for `snapshot` as SVG, with `count` levels spread over
/// the gradient's range (the user's `range` if given), colored like the raster map.
pub fn contours_svg(
    zone: &Zone,
    snapshot: &Snapshot,
    metric: &Metric,
    preset: GradientPreset,
    range: Option<(f32, f32)>,
    count: usize,
) -> Result<String, ExportError> {
    let map = zone.map.as_ref().ok_or(ExportError::NoMap)?;
    if metric.palette().is_some() {
        return Err(ExportError::NotContinuous);
    }
    let gradient = effective_gradient(metric, preset, range);
    let (lo, hi) = gradient.range();
    let lines = isolines(&samples(snapshot, metric), &evenly_spaced_levels(lo, hi, count));
    let unit = metric.unit();
    Ok(isolines_to_svg(f64::from(map.width), f64::from(map.height), &lines, |l| gradient.color(l), |l| format!("{l:.0} {unit}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_heatmap::Polyline;
    use openspot_model::{MapImage, ScanPoint};
    use openspot_wlan::{AccessPoint, MacAddress};

    #[test]
    fn writes_open_and_closed_paths_with_escaped_titles() {
        let lines = vec![Isoline {
            level: -60.0,
            lines: vec![
                Polyline { points: vec![(0.0, 0.0), (10.0, 5.5), (0.0, 0.0)], closed: true },
                Polyline { points: vec![(1.0, 2.0), (3.0, 4.0)], closed: false },
            ],
        }];
        let svg = isolines_to_svg(200.0, 100.0, &lines, |_| [1, 2, 3], |_| "a<b & c".into());
        assert!(svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100""#));
        assert_eq!(svg.matches("<path").count(), 2);
        assert!(svg.contains(r#"d="M0.00 0.00 L10.00 5.50 Z""#), "closed path drops the repeated point: {svg}");
        assert!(svg.contains(r#"d="M1.00 2.00 L3.00 4.00""#));
        assert!(svg.contains("rgb(1,2,3)"));
        assert!(svg.contains("<title>a&lt;b &amp; c</title>"));
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    fn zone_and_snapshot() -> (Zone, Snapshot) {
        let mut zone = Zone::new("z");
        zone.map = Some(MapImage { original_name: "m".into(), data: vec![], width: 300, height: 200 });
        let mut snap = Snapshot::new("s");
        let ap = |rssi| AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, 1]), rssi, 2_437_000, true, 100, vec![]);
        // Signal falls linearly from -40 dBm at x = 0 to -90 dBm at x = 300.
        for (x, y) in [(0.0, 0.0), (0.0, 200.0), (150.0, 0.0), (150.0, 200.0), (300.0, 0.0), (300.0, 200.0)] {
            snap.points.push(ScanPoint::new(x, y, 0, vec![ap(-40 - (x / 6.0) as i32)]));
        }
        (zone, snap)
    }

    #[test]
    fn contours_svg_draws_vertical_lines_at_the_right_x() {
        let (zone, snap) = zone_and_snapshot();
        let metric = Metric::Signal(vec![]);
        let svg = contours_svg(&zone, &snap, &metric, GradientPreset::Default, Some((-90.0, -40.0)), 4).unwrap();
        // Levels -80, -70, -60, -50 dBm sit at x = 240, 180, 120, 60.
        for x in [240, 180, 120, 60] {
            assert!(svg.contains(&format!("M{x}.00 ")) || svg.contains(&format!("L{x}.00 ")), "x={x}\n{svg}");
        }
        assert_eq!(svg.matches("<path").count(), 4);
        assert!(svg.contains("-70 dBm"));
        assert!(svg.contains(r#"viewBox="0 0 300 200""#));
    }

    #[test]
    fn categorical_metrics_and_missing_plans_are_rejected() {
        let (zone, snap) = zone_and_snapshot();
        let cat = Metric::BandCoverage { wanted: vec![], min_rssi_dbm: -80 };
        assert!(matches!(contours_svg(&zone, &snap, &cat, GradientPreset::Default, None, 3), Err(ExportError::NotContinuous)));
        let bare = Zone::new("b");
        assert!(matches!(contours_svg(&bare, &snap, &Metric::Signal(vec![]), GradientPreset::Default, None, 3), Err(ExportError::NoMap)));
    }
}
