//! Choosing and coloring what a finished prediction shows (view modes of P-10).
//!
//! Pure: a [`Prediction`] and [`ViewSettings`] give a [`ViewImage`] (value grid, RGBA pixels,
//! legend). The canvas only uploads the pixels.

use openspot_heatmap::{Gradient, GradientPreset, Grid, Metric, Smoothing, colorize_ranged, effective_gradient, smooth};
use openspot_model::{Band, Uuid};

use super::predict_input::Prediction;
use crate::heat_ui;

/// A radio counts toward "number of APs" when heard at least this loudly: weaker signals
/// are not usable by a client. Same figure as Survey's AP-count map.
pub const AP_COUNT_MIN_RSSI_DBM: f64 = -80.0;
/// Radios weaker than this are ignored by the SIR view: below roughly the receiver
/// sensitivity of a basic rate, a frame cannot be decoded, so the radio neither serves nor is
/// worth counting as a co-channel contender.
pub const SIR_HEARD_DBM: f64 = -90.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Loudest signal from any radio.
    Strongest,
    /// One radio's signal.
    Radio { ap: Uuid, radio_index: usize },
    /// How many radios are usable.
    ApCount,
    /// Co-channel signal-to-interference.
    Sir,
    /// Which radio serves each point.
    Serving,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewSettings {
    pub mode: ViewMode,
    /// Only radios of this band, or all.
    pub band: Option<Band>,
    pub gradient: GradientPreset,
    pub smoothing: Smoothing,
    pub opacity: u8,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self { mode: ViewMode::Strongest, band: None, gradient: GradientPreset::Default, smoothing: Smoothing::Off, opacity: 170 }
    }
}

/// What the legend shows.
#[derive(Debug, Clone)]
pub enum Legend {
    Gradient { gradient: Gradient, unit: &'static str },
    /// One entry per radio (RGB and label).
    Radios(Vec<([u8; 3], String)>),
}

#[derive(Debug, Clone)]
pub struct ViewImage {
    pub grid: Grid,
    pub rgba: Vec<u8>,
    pub legend: Legend,
    metric: Option<Metric>,
    labels: Vec<String>,
}

fn in_scope(band: Option<Band>, b: Band) -> bool {
    band.is_none_or(|x| x == b)
}

/// Color of radio `index` of `count`: hue by access point (golden-ratio spacing keeps
/// neighbours distinct), brightness by band so one AP's radios read as a family.
fn radio_color(pred: &Prediction, index: usize) -> [u8; 3] {
    let info = &pred.radios[index];
    let mut aps: Vec<Uuid> = Vec::new();
    for r in &pred.radios {
        if !aps.contains(&r.ap_id) {
            aps.push(r.ap_id);
        }
    }
    let n = aps.iter().position(|a| *a == info.ap_id).unwrap_or(0);
    let hue = (n as f32 * 0.618_034).fract();
    let value = match info.band {
        Band::Ghz2_4 => 0.95,
        Band::Ghz5 => 0.78,
        Band::Ghz6 => 0.62,
    };
    hsv_to_rgb(hue, 0.65, value)
}

/// HSV (all `0..=1`) to RGB.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match (i as i32).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b].map(|c| (c * 255.0).round() as u8)
}

/// Pixels and legend for `view` of `pred`, or `None` when no radio is in scope.
pub fn render(pred: &Prediction, view: &ViewSettings) -> Option<ViewImage> {
    let band = view.band;
    let radios_in_scope = pred.radios.iter().any(|r| in_scope(band, r.band));
    let continuous = |grid: Grid, metric: Metric| {
        let grid = if view.smoothing == Smoothing::Off { grid } else { smooth(&grid, view.smoothing.radius_cells()) };
        let rgba = colorize_ranged(&metric, &grid, view.opacity, view.gradient, None);
        let legend = Legend::Gradient { gradient: effective_gradient(&metric, view.gradient, None), unit: metric.unit() };
        Some(ViewImage { grid, rgba, legend, metric: Some(metric), labels: Vec::new() })
    };
    match view.mode {
        ViewMode::Strongest if radios_in_scope => continuous(pred.set.strongest(band), Metric::Signal(Vec::new())),
        ViewMode::Radio { ap, radio_index } => {
            let found = pred.set.radios.iter().find(|r| r.ap_id == ap && r.radio_index == radio_index)?;
            continuous(found.grid.clone(), Metric::Signal(Vec::new()))
        }
        ViewMode::Sir if radios_in_scope => continuous(pred.set.sir(SIR_HEARD_DBM, band), Metric::Sir(Vec::new())),
        ViewMode::ApCount if radios_in_scope => {
            let metric = Metric::ApCount { min_rssi_dbm: AP_COUNT_MIN_RSSI_DBM as i32 };
            let grid = pred.set.ap_count(AP_COUNT_MIN_RSSI_DBM, band);
            let rgba = colorize_ranged(&metric, &grid, view.opacity, view.gradient, None);
            let legend = Legend::Gradient { gradient: effective_gradient(&metric, view.gradient, None), unit: metric.unit() };
            Some(ViewImage { grid, rgba, legend, metric: Some(metric), labels: Vec::new() })
        }
        ViewMode::Serving if radios_in_scope => {
            let grid = pred.set.serving_radio(band);
            let colors: Vec<[u8; 3]> = (0..pred.radios.len()).map(|i| radio_color(pred, i)).collect();
            let rgba = grid
                .values
                .iter()
                .flat_map(|v| match v {
                    Some(i) => {
                        let [r, g, b] = colors.get(*i as usize).copied().unwrap_or([128, 128, 128]);
                        [r, g, b, view.opacity]
                    }
                    None => [0, 0, 0, 0],
                })
                .collect();
            let labels: Vec<String> = pred.radios.iter().map(|r| r.label()).collect();
            let legend = Legend::Radios(pred.radios.iter().enumerate().filter(|(_, r)| in_scope(band, r.band)).map(|(i, r)| (colors[i], r.label())).collect());
            Some(ViewImage { grid, rgba, legend, metric: None, labels })
        }
        _ => None,
    }
}

/// The radio `delta` places after (or before, if negative) the one `current` shows, wrapping
/// around; starts at the first radio if `current` is not a per-radio view. `None` without radios.
pub fn cycle_radio(pred: &Prediction, current: ViewMode, delta: isize) -> Option<ViewMode> {
    let n = pred.radios.len() as isize;
    if n == 0 {
        return None;
    }
    let at = match current {
        ViewMode::Radio { ap, radio_index } => pred.radios.iter().position(|r| r.ap_id == ap && r.radio_index == radio_index).map_or(0, |i| i as isize),
        _ => 0,
    };
    let next = &pred.radios[(at + delta).rem_euclid(n) as usize];
    Some(ViewMode::Radio { ap: next.ap_id, radio_index: next.radio_index })
}

impl ViewImage {
    /// Hover text for map position `(x, y)` in pixels.
    pub fn text_at(&self, x: f64, y: f64) -> Option<String> {
        let value = heat_ui::value_at(&self.grid, x, y)?;
        match &self.metric {
            Some(metric) => Some(heat_ui::value_label(metric, value)),
            None => self.labels.get(value.round().max(0.0) as usize).cloned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::ap_models::builtin_models;
    use crate::planning::predict_input::tests::two_room_project;
    use crate::planning::predict_input::{PredictSettings, build_input, compute};
    use openspot_model::{MaterialCatalog, Point};
    use openspot_predict::Control;

    /// Two APs (2.4 + 5 GHz each) on both sides of the dividing wall.
    fn prediction() -> Prediction {
        let mut p = two_room_project("concrete-wall");
        let mut model = builtin_models().remove(0);
        model.radios.retain(|r| r.band == Band::Ghz2_4 || r.band == Band::Ghz5);
        p.zones[0].planning.access_points.clear();
        for (name, x) in [("West", 3.0), ("East", 17.0)] {
            let mut ap = model.instantiate(name, Point::new(x, 5.0));
            ap.height_m = 1.0;
            p.zones[0].planning.access_points.push(ap);
        }
        let input = build_input(&p, 0, &MaterialCatalog::builtin().unwrap(), &PredictSettings::default()).unwrap();
        compute(&input, &Control::default()).unwrap().unwrap()
    }

    fn view(mode: ViewMode, band: Option<Band>) -> ViewSettings {
        ViewSettings { mode, band, opacity: 200, ..ViewSettings::default() }
    }

    fn cell_at(img: &ViewImage, x_m: f64, y_m: f64) -> f32 {
        heat_ui::value_at(&img.grid, x_m * 50.0, y_m * 50.0).unwrap()
    }

    #[test]
    fn strongest_is_the_cellwise_maximum_per_band() {
        let pred = prediction();
        let all = render(&pred, &view(ViewMode::Strongest, None)).unwrap();
        let g5 = render(&pred, &view(ViewMode::Strongest, Some(Band::Ghz5))).unwrap();
        let g24 = render(&pred, &view(ViewMode::Strongest, Some(Band::Ghz2_4))).unwrap();
        for (i, v) in all.grid.values.iter().enumerate() {
            assert_eq!(*v, Some(g5.grid.values[i].unwrap().max(g24.grid.values[i].unwrap())));
        }
        let Legend::Gradient { gradient, unit } = &all.legend else { panic!("gradient legend") };
        assert_eq!((gradient.range(), *unit), ((-90.0, -35.0), "dBm"));
        assert_eq!(all.rgba.len(), all.grid.values.len() * 4);
        assert!(all.rgba.chunks(4).all(|px| px[3] == 200), "every cell has a value, drawn at the chosen opacity");
        assert!(render(&pred, &view(ViewMode::Strongest, Some(Band::Ghz6))).is_none(), "no 6 GHz radio: nothing to show");
    }

    #[test]
    fn per_radio_view_is_that_radios_own_grid_and_the_two_sides_differ() {
        let pred = prediction();
        let west = &pred.radios[0];
        assert_eq!(west.ap_name, "West");
        let img = render(&pred, &view(ViewMode::Radio { ap: west.ap_id, radio_index: west.radio_index }, None)).unwrap();
        assert_eq!(img.grid, pred.set.radios[0].grid);
        assert!(cell_at(&img, 4.0, 5.0) > cell_at(&img, 16.0, 5.0) + 15.0, "far side of a concrete wall is much weaker");
        assert!(render(&pred, &view(ViewMode::Radio { ap: Uuid::new_v4(), radio_index: 0 }, None)).is_none(), "unknown radio");
        assert_eq!(img.text_at(4.0 * 50.0, 5.0 * 50.0).unwrap().split(' ').nth(1), Some("dBm"));
    }

    #[test]
    fn ap_count_counts_radios_above_the_threshold() {
        let pred = prediction();
        let g5 = render(&pred, &view(ViewMode::ApCount, Some(Band::Ghz5))).unwrap();
        // Analytic level of the far AP at the cell center (3.05, 5.05) m: 13.95 m away through
        // the 5 GHz concrete wall, default exponent 2.5, 20 dBm + 4 dBi at 5210 MHz.
        let wall = MaterialCatalog::builtin().unwrap().get("concrete-wall").unwrap().absorption_db.ghz5;
        let far = 24.0 - (openspot_predict::fspl_db(1.0, 5210.0) + 25.0 * 13.95f64.log10() + wall);
        assert!((-80.0..-75.0).contains(&far), "the scenario puts the far AP just above the threshold: {far}");
        assert_eq!(cell_at(&g5, 3.0, 5.0), 2.0, "near AP plus the far AP at {far:.1} dBm");
        // Across the whole 2.4 GHz grid the count is exactly the radios at or above -80 dBm.
        let g24 = render(&pred, &view(ViewMode::ApCount, Some(Band::Ghz2_4))).unwrap();
        for (i, v) in g24.grid.values.iter().enumerate() {
            let loud = pred.set.radios.iter().filter(|r| r.band == Band::Ghz2_4).filter(|r| f64::from(r.grid.values[i].unwrap()) >= AP_COUNT_MIN_RSSI_DBM).count();
            assert_eq!(*v, Some(loud as f32));
        }
        let both = render(&pred, &view(ViewMode::ApCount, None)).unwrap();
        assert!(cell_at(&both, 3.0, 5.0) >= cell_at(&g5, 3.0, 5.0), "adding the 2.4 GHz radios never lowers the count");
        assert_eq!(g5.text_at(3.0 * 50.0, 5.0 * 50.0).unwrap(), "2 APs");
    }

    #[test]
    fn sir_is_capped_without_interferers_and_drops_with_a_co_channel_neighbour() {
        let pred = prediction();
        // Both APs use 5 GHz channel 42 (80 MHz): at the wall both are heard.
        let sir = render(&pred, &view(ViewMode::Sir, Some(Band::Ghz5))).unwrap();
        let near_west = cell_at(&sir, 3.0, 5.0);
        let near_wall = cell_at(&sir, 9.5, 5.0);
        assert!(near_wall < near_west, "{near_wall} < {near_west}: contention grows toward the other AP");
        assert!(near_west <= openspot_heatmap::MAX_SIR_DB as f32);
        let Legend::Gradient { gradient, unit } = &sir.legend else { panic!("gradient legend") };
        assert_eq!((gradient.range(), *unit), ((0.0, 35.0), "dB"));
    }

    #[test]
    fn serving_view_names_the_radio_and_colors_by_access_point() {
        let pred = prediction();
        let img = render(&pred, &view(ViewMode::Serving, Some(Band::Ghz5))).unwrap();
        assert_eq!(img.text_at(3.0 * 50.0, 5.0 * 50.0).unwrap(), "West · 5 GHz ch 42");
        assert_eq!(img.text_at(17.0 * 50.0, 5.0 * 50.0).unwrap(), "East · 5 GHz ch 42");
        let Legend::Radios(entries) = &img.legend else { panic!("radio legend") };
        assert_eq!(entries.len(), 2, "only the 5 GHz radios are in scope");
        assert_ne!(entries[0].0, entries[1].0);
        // The pixel of a cell is its serving radio's legend color, at the chosen opacity.
        let i = (5.0 * 50.0 / img.grid.cell_px) as usize * img.grid.cols + (3.0 * 50.0 / img.grid.cell_px) as usize;
        assert_eq!(&img.rgba[i * 4..i * 4 + 4], &[entries[0].0[0], entries[0].0[1], entries[0].0[2], 200]);
        // Both bands in scope: one AP's two radios share a hue but differ in brightness.
        let both = render(&pred, &view(ViewMode::Serving, None)).unwrap();
        let Legend::Radios(all) = &both.legend else { panic!("radio legend") };
        assert_eq!(all.len(), 4);
        assert_ne!(all[0].0, all[1].0);
    }

    #[test]
    fn smoothing_changes_continuous_views_but_never_categories_or_counts() {
        let pred = prediction();
        let smooth_view = |mode| ViewSettings { smoothing: Smoothing::Strong, ..view(mode, Some(Band::Ghz5)) };
        let sharp = render(&pred, &view(ViewMode::Strongest, Some(Band::Ghz5))).unwrap();
        let soft = render(&pred, &smooth_view(ViewMode::Strongest)).unwrap();
        assert_ne!(sharp.grid, soft.grid);
        assert_eq!(soft.grid.values.len(), sharp.grid.values.len());
        for mode in [ViewMode::ApCount, ViewMode::Serving] {
            assert_eq!(render(&pred, &smooth_view(mode)).unwrap().grid, render(&pred, &view(mode, Some(Band::Ghz5))).unwrap().grid, "{mode:?}");
        }
    }

    #[test]
    fn gradient_preset_changes_the_pixels_not_the_values() {
        let pred = prediction();
        let a = render(&pred, &view(ViewMode::Strongest, None)).unwrap();
        let b = render(&pred, &ViewSettings { gradient: GradientPreset::BlueRed, ..view(ViewMode::Strongest, None) }).unwrap();
        assert_eq!(a.grid, b.grid);
        assert_ne!(a.rgba, b.rgba);
    }

    #[test]
    fn cycling_walks_the_radios_and_wraps() {
        let pred = prediction();
        let first = cycle_radio(&pred, ViewMode::Strongest, 0).unwrap();
        let r = &pred.radios;
        assert_eq!(first, ViewMode::Radio { ap: r[0].ap_id, radio_index: r[0].radio_index });
        let second = cycle_radio(&pred, first, 1).unwrap();
        assert_eq!(second, ViewMode::Radio { ap: r[1].ap_id, radio_index: r[1].radio_index });
        assert_eq!(cycle_radio(&pred, first, -1).unwrap(), ViewMode::Radio { ap: r[3].ap_id, radio_index: r[3].radio_index }, "backwards from the first wraps to the last");
        let mut mode = first;
        for _ in 0..r.len() {
            mode = cycle_radio(&pred, mode, 1).unwrap();
        }
        assert_eq!(mode, first, "a full lap returns to the start");
        let empty = Prediction { radios: vec![], ..pred };
        assert_eq!(cycle_radio(&empty, ViewMode::Strongest, 1), None);
    }

    #[test]
    fn hsv_matches_primary_colors() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv_to_rgb(1.0 / 3.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hsv_to_rgb(2.0 / 3.0, 1.0, 1.0), [0, 0, 255]);
        assert_eq!(hsv_to_rgb(0.5, 0.0, 0.5), [128, 128, 128]);
    }
}
