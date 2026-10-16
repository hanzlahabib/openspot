//! Vector isolines of the active heatmap (H-16): computation cache and SVG export.

use openspot_heatmap::{Isoline, evenly_spaced_levels, isolines, samples};

use super::{HeatKey, Survey};

/// Levels drawn by default: enough to read the gradient without cluttering the plan.
pub const DEFAULT_CONTOUR_LEVELS: usize = 5;
/// Most levels the UI offers; beyond this the lines merge into a wash.
pub const MAX_CONTOUR_LEVELS: usize = 12;

/// Cached isolines with the inputs they were computed from.
#[derive(Debug, Default)]
pub struct ContourCache {
    key: Option<(HeatKey, usize)>,
    lines: Vec<Isoline>,
}

impl Survey {
    /// Isolines for the current snapshot/metric, recomputed only when an input changed.
    /// Empty for categorical metrics, which have no scale to draw levels on.
    pub fn contour_lines(&mut self) -> &[Isoline] {
        let key = (self.heat_key(), self.contour_levels);
        if self.contours.key.as_ref() != Some(&key) {
            let metric = self.metric();
            self.contours.lines = if metric.palette().is_some() {
                Vec::new()
            } else {
                let (lo, hi) = self.effective_range();
                isolines(&samples(self.snapshot(), &metric), &evenly_spaced_levels(lo, hi, self.contour_levels))
            };
            self.contours.key = Some(key);
        }
        &self.contours.lines
    }

    /// SVG text of the current contours.
    pub fn contours_svg(&self) -> Result<String, openspot_export::ExportError> {
        openspot_export::contours_svg(self.zone(), self.snapshot(), &self.metric(), self.gradient, self.active_range(), self.contour_levels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survey::MetricChoice;
    use openspot_wlan::{AccessPoint, MacAddress};

    fn survey() -> Survey {
        let mut s = Survey::default();
        s.create_blank_map(10.0, 10.0).unwrap();
        let ap = |rssi| AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, 1]), rssi, 2_437_000, true, 100, vec![]);
        for (x, y, rssi) in [(0.0, 0.0, -40), (0.0, 1600.0, -40), (1600.0, 0.0, -90), (1600.0, 1600.0, -90)] {
            s.add_scan(x, y, vec![ap(rssi)]);
        }
        s
    }

    #[test]
    fn lines_are_cached_until_an_input_changes() {
        let mut s = survey();
        s.contour_levels = 4;
        assert_eq!(s.contour_lines().len(), 4);
        assert!(s.contour_lines().iter().all(|i| !i.lines.is_empty()));
        // Mutate the cache's result directly: an unchanged key must keep it.
        s.contours.lines.clear();
        assert!(s.contour_lines().is_empty(), "served from cache");
        s.contour_levels = 3;
        assert_eq!(s.contour_lines().len(), 3, "level count change recomputes");
        s.add_scan(800.0, 800.0, vec![]);
        s.contours.lines.clear();
        assert_eq!(s.contour_lines().len(), 3, "new measurement recomputes");
    }

    #[test]
    fn categorical_metric_has_no_contours_and_svg_errors() {
        let mut s = survey();
        s.metric_choice = MetricChoice::BandCoverage;
        assert!(s.contour_lines().is_empty());
        assert!(s.contours_svg().is_err());
        s.metric_choice = MetricChoice::BestSignal;
        assert!(s.contours_svg().unwrap().contains("<path"));
    }
}
