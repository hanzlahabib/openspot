/// Piecewise-linear color ramp over a value range.
#[derive(Debug, Clone)]
pub struct Gradient {
    /// `(value, rgb)` stops, ascending by value.
    stops: Vec<(f32, [u8; 3])>,
}

/// Color scheme for continuous heatmaps. `Default` keeps each metric's own ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientPreset {
    #[default]
    Default,
    /// Red (low) through yellow to green (high).
    RedGreen,
    /// Blue (low) through cyan and yellow to red (high).
    BlueRed,
}

impl GradientPreset {
    pub const ALL: [Self; 3] = [Self::Default, Self::RedGreen, Self::BlueRed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::RedGreen => "Red to green",
            Self::BlueRed => "Blue to red",
        }
    }

    /// Ramp over `[0, 1]`, or `None` for the metric's own colors.
    fn ramp(self) -> Option<Gradient> {
        match self {
            Self::Default => None,
            Self::RedGreen => Some(Gradient::new(vec![(0.0, [215, 25, 28]), (0.5, [255, 221, 80]), (1.0, [26, 150, 65])])),
            Self::BlueRed => Some(Gradient::new(vec![
                (0.0, [49, 54, 149]),
                (0.35, [116, 173, 209]),
                (0.65, [254, 224, 144]),
                (1.0, [165, 0, 38]),
            ])),
        }
    }
}

impl Gradient {
    /// The value range covered by the stops.
    pub fn range(&self) -> (f32, f32) {
        (self.stops[0].0, self.stops[self.stops.len() - 1].0)
    }

    /// Same value stops with the preset's colors spread across them proportionally.
    pub fn with_preset(self, preset: GradientPreset) -> Self {
        let Some(ramp) = preset.ramp() else { return self };
        let (lo, hi) = self.range();
        let span = (hi - lo).max(f32::EPSILON);
        Self { stops: self.stops.iter().map(|&(v, _)| (v, ramp.color((v - lo) / span))).collect() }
    }

    /// Same colors with the value stops spread linearly over `lo..hi` (a user-chosen
    /// range). An empty or non-finite range leaves the gradient unchanged.
    pub fn rescale(self, lo: f32, hi: f32) -> Self {
        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            return self;
        }
        let (old_lo, old_hi) = self.range();
        let span = old_hi - old_lo;
        if span <= 0.0 {
            // A single-stop gradient has no scale to stretch.
            return self;
        }
        Self { stops: self.stops.iter().map(|&(v, c)| (lo + (v - old_lo) / span * (hi - lo), c)).collect() }
    }

    pub fn new(stops: Vec<(f32, [u8; 3])>) -> Self {
        assert!(!stops.is_empty() && stops.windows(2).all(|w| w[0].0 < w[1].0), "stops must ascend");
        Self { stops }
    }

    /// Red (unusable) to green (excellent) over typical Wi-Fi RSSI, dBm.
    pub fn signal_dbm() -> Self {
        Self::new(vec![
            (-90.0, [215, 25, 28]),
            (-75.0, [253, 141, 60]),
            (-67.0, [255, 221, 80]),
            (-55.0, [166, 217, 106]),
            (-35.0, [26, 150, 65]),
        ])
    }

    /// Red (interference dominates) to green (clean channel) over a dB ratio, such as SIR.
    pub fn ratio_db() -> Self {
        Self::new(vec![
            (0.0, [215, 25, 28]),
            (10.0, [253, 141, 60]),
            (15.0, [255, 221, 80]),
            (25.0, [166, 217, 106]),
            (35.0, [26, 150, 65]),
        ])
    }

    /// Low counts cool, high counts hot, up to `max`.
    pub fn count(max: u32) -> Self {
        let max = max.max(2) as f32;
        Self::new(vec![(0.0, [49, 54, 149]), (max / 2.0, [255, 255, 191]), (max, [165, 0, 38])])
    }

    pub fn color(&self, value: f32) -> [u8; 3] {
        let (first, last) = (self.stops[0], self.stops[self.stops.len() - 1]);
        if value <= first.0 {
            return first.1;
        }
        if value >= last.0 {
            return last.1;
        }
        let i = self.stops.partition_point(|s| s.0 <= value);
        let ((v0, c0), (v1, c1)) = (self.stops[i - 1], self.stops[i]);
        let t = (value - v0) / (v1 - v0);
        std::array::from_fn(|k| (f32::from(c0[k]) + t * (f32::from(c1[k]) - f32::from(c0[k]))).round() as u8)
    }

    /// RGBA8 pixels (row-major) for `grid`; cells without data are fully transparent.
    pub fn render_rgba(&self, grid: &crate::Grid, alpha: u8) -> Vec<u8> {
        grid.values
            .iter()
            .flat_map(|v| match v {
                Some(v) => {
                    let [r, g, b] = self.color(*v);
                    [r, g, b, alpha]
                }
                None => [0, 0, 0, 0],
            })
            .collect()
    }
}

/// Labelled flat colors for categorical maps; category `i` uses entry `i`.
#[derive(Debug, Clone)]
pub struct Palette {
    entries: Vec<(&'static str, [u8; 3])>,
}

impl Palette {
    pub fn new(entries: Vec<(&'static str, [u8; 3])>) -> Self {
        assert!(!entries.is_empty(), "palette needs at least one entry");
        Self { entries }
    }

    pub fn entries(&self) -> &[(&'static str, [u8; 3])] {
        &self.entries
    }

    /// Out-of-range categories clamp to the last entry.
    pub fn color(&self, category: f32) -> [u8; 3] {
        let i = (category.round().max(0.0) as usize).min(self.entries.len() - 1);
        self.entries[i].1
    }

    pub fn render_rgba(&self, grid: &crate::Grid, alpha: u8) -> Vec<u8> {
        grid.values
            .iter()
            .flat_map(|v| match v {
                Some(v) => {
                    let [r, g, b] = self.color(*v);
                    [r, g, b, alpha]
                }
                None => [0, 0, 0, 0],
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Grid;

    #[test]
    fn clamps_and_interpolates() {
        let g = Gradient::signal_dbm();
        assert_eq!(g.color(-120.0), [215, 25, 28]);
        assert_eq!(g.color(-10.0), [26, 150, 65]);
        assert_eq!(g.color(-75.0), [253, 141, 60]);
        let mid = g.color(-82.5);
        assert!(mid[0] > 215 - 1 && mid[0] < 253 + 1);
    }

    #[test]
    fn presets_recolor_but_keep_value_stops() {
        let base = Gradient::signal_dbm();
        assert_eq!(base.clone().with_preset(GradientPreset::Default).color(-90.0), base.color(-90.0));
        let rg = Gradient::signal_dbm().with_preset(GradientPreset::RedGreen);
        assert_eq!(rg.range(), base.range());
        assert_eq!(rg.color(-90.0), [215, 25, 28]); // low end red
        assert_eq!(rg.color(-35.0), [26, 150, 65]); // high end green
        let br = Gradient::signal_dbm().with_preset(GradientPreset::BlueRed);
        assert_eq!(br.color(-90.0), [49, 54, 149]);
        assert_eq!(br.color(-35.0), [165, 0, 38]);
        assert_eq!(GradientPreset::ALL.len(), 3);
    }

    #[test]
    fn rescale_moves_stops_but_keeps_colors_at_the_same_relative_position() {
        let base = Gradient::signal_dbm();
        let (lo, hi) = base.range();
        let r = Gradient::signal_dbm().rescale(-100.0, -20.0);
        assert_eq!(r.range(), (-100.0, -20.0));
        // Same relative position, same color.
        for rel in [0.0f32, 0.25, 0.5, 0.8, 1.0] {
            assert_eq!(r.color(-100.0 + rel * 80.0), base.color(lo + rel * (hi - lo)), "rel {rel}");
        }
        // A value that used to be mid-ramp is now beyond the top and clamps.
        assert_eq!(Gradient::signal_dbm().rescale(-100.0, -90.0).color(-60.0), base.color(hi));
    }

    #[test]
    fn rescale_ignores_invalid_ranges() {
        for (lo, hi) in [(5.0, 5.0), (6.0, 5.0), (f32::NAN, 1.0), (0.0, f32::INFINITY)] {
            assert_eq!(Gradient::signal_dbm().rescale(lo, hi).range(), Gradient::signal_dbm().range());
        }
        let single = Gradient::new(vec![(1.0, [1, 2, 3])]);
        assert_eq!(single.rescale(0.0, 10.0).range(), (1.0, 1.0));
    }

    #[test]
    fn renders_transparent_holes() {
        let grid = Grid { cols: 2, rows: 1, cell_px: 1.0, values: vec![Some(-40.0), None] };
        let px = Gradient::signal_dbm().render_rgba(&grid, 180);
        assert_eq!(px.len(), 8);
        assert_eq!(px[3], 180);
        assert_eq!(&px[4..], &[0, 0, 0, 0]);
    }

    #[test]
    fn palette_clamps_and_renders() {
        let p = Palette::new(vec![("a", [1, 2, 3]), ("b", [4, 5, 6])]);
        assert_eq!(p.color(0.0), [1, 2, 3]);
        assert_eq!(p.color(1.2), [4, 5, 6]);
        assert_eq!(p.color(9.0), [4, 5, 6]);
        assert_eq!(p.color(-3.0), [1, 2, 3]);
        let grid = Grid { cols: 2, rows: 1, cell_px: 1.0, values: vec![Some(1.0), None] };
        assert_eq!(p.render_rgba(&grid, 99), vec![4, 5, 6, 99, 0, 0, 0, 0]);
    }

    #[test]
    #[should_panic(expected = "ascend")]
    fn rejects_unsorted_stops() {
        Gradient::new(vec![(1.0, [0; 3]), (0.0, [0; 3])]);
    }
}
