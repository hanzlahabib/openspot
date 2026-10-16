//! User-defined value range for continuous gradients (H-15).

use openspot_heatmap::metric_key;

use super::Survey;

/// Narrowest span accepted: below this the gradient collapses to a hard edge, which is
/// never what someone adjusting a legend wants.
pub const MIN_SPAN: f32 = 1.0;

pub fn is_valid((lo, hi): (f32, f32)) -> bool {
    lo.is_finite() && hi.is_finite() && hi - lo >= MIN_SPAN
}

impl Survey {
    /// The user's range for the current metric, if one is set and the metric is continuous.
    pub fn active_range(&self) -> Option<(f32, f32)> {
        let metric = self.metric();
        if metric.palette().is_some() {
            return None;
        }
        self.value_ranges.get(&metric_key(&metric)).copied()
    }

    /// The range currently in force: the user's, else the metric's own.
    pub fn effective_range(&self) -> (f32, f32) {
        self.active_range().unwrap_or_else(|| self.metric().gradient().range())
    }

    /// Sets the range for the current metric. Invalid ranges are rejected.
    pub fn set_range(&mut self, range: (f32, f32)) -> bool {
        let metric = self.metric();
        if metric.palette().is_some() || !is_valid(range) {
            return false;
        }
        self.value_ranges.insert(metric_key(&metric), range);
        // The texture bakes the colors in; the generation bump forces a rebuild.
        self.generation += 1;
        true
    }

    pub fn reset_range(&mut self) {
        if self.value_ranges.remove(&metric_key(&self.metric())).is_some() {
            self.generation += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survey::MetricChoice;

    #[test]
    fn range_is_per_metric_validated_and_resettable() {
        let mut s = Survey::default();
        assert_eq!(s.active_range(), None);
        assert_eq!(s.effective_range(), (-90.0, -35.0));
        assert!(s.set_range((-100.0, -50.0)));
        assert_eq!(s.active_range(), Some((-100.0, -50.0)));
        assert_eq!(s.effective_range(), (-100.0, -50.0));
        // Another metric keeps its own range.
        s.metric_choice = MetricChoice::ApCount;
        assert_eq!(s.active_range(), None);
        // Same dBm scale for every signal variant.
        s.metric_choice = MetricChoice::SelectedSignal;
        assert_eq!(s.active_range(), Some((-100.0, -50.0)));
        s.metric_choice = MetricChoice::BestSignal;
        assert!(!s.set_range((-50.0, -50.0)));
        assert!(!s.set_range((-60.0, f32::NAN)));
        assert_eq!(s.active_range(), Some((-100.0, -50.0)), "bad input leaves it alone");
        s.reset_range();
        assert_eq!(s.active_range(), None);
    }

    #[test]
    fn categorical_metrics_ignore_ranges() {
        let mut s = Survey { metric_choice: MetricChoice::BandCoverage, ..Survey::default() };
        assert!(!s.set_range((0.0, 10.0)));
        assert_eq!(s.active_range(), None);
    }

    #[test]
    fn setting_a_range_invalidates_the_heatmap_cache_and_persists_through_prefs() {
        let mut s = Survey::default();
        let key = s.heat_key();
        s.set_range((-95.0, -45.0));
        assert_ne!(s.heat_key(), key);
        let prefs = s.prefs();
        let mut fresh = Survey::default();
        fresh.apply_prefs(&prefs);
        assert_eq!(fresh.active_range(), Some((-95.0, -45.0)));
        // Invalid stored ranges are dropped rather than trusted.
        let mut bad = prefs;
        bad.value_ranges.insert("Signal".into(), (5.0, 5.0));
        fresh.apply_prefs(&bad);
        assert_eq!(fresh.active_range(), None);
    }

    #[test]
    fn export_uses_the_same_range_as_the_screen() {
        let mut s = Survey::default();
        s.create_blank_map(10.0, 10.0).unwrap();
        s.add_scan(100.0, 100.0, vec![openspot_wlan::AccessPoint::from_beacon("n".into(), openspot_wlan::MacAddress([0, 0, 0, 0, 0, 1]), -60, 2_437_000, true, 100, vec![])]);
        s.add_scan(1500.0, 700.0, vec![openspot_wlan::AccessPoint::from_beacon("n".into(), openspot_wlan::MacAddress([0, 0, 0, 0, 0, 1]), -80, 2_437_000, true, 100, vec![])]);
        let render = |s: &Survey| {
            let style = openspot_export::PngStyle { preset: s.gradient, range: s.active_range(), smoothing: s.smoothing, alpha: 255, show_points: false };
            openspot_export::render_png_styled(s.zone(), s.snapshot(), &s.metric(), &style).unwrap()
        };
        let before = render(&s);
        s.set_range((-85.0, -55.0));
        assert_ne!(render(&s), before);
    }
}
