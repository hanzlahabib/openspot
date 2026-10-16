//! Heatmap computation on a background thread (H-19). The UI asks [`Survey::advance_heat`]
//! every frame; it starts a job when the inputs changed, cancels a stale one, and hands
//! back the result when it is ready. The previous texture stays on screen meanwhile.

use openspot_heatmap::{Control, Grid, GridSpec, Job, JobPoll, colorize_ranged, rasterize_samples, samples};

use super::{HEATMAP_LONG_SIDE_CELLS, HeatKey, Survey};

/// Everything the canvas needs to show a finished heatmap.
#[derive(Debug, PartialEq)]
pub struct HeatOutput {
    pub grid: Grid,
    /// RGBA pixels for `grid`, baked with `alpha`.
    pub rgba: Vec<u8>,
    /// Opacity the pixels were colored with; the user may have moved the slider since the job started.
    pub alpha: u8,
}

#[derive(Debug)]
struct Running {
    key: HeatKey,
    job: Job<Option<HeatOutput>>,
}

/// Background-render bookkeeping.
#[derive(Debug, Default)]
pub struct HeatJobs {
    running: Option<Running>,
    /// Inputs of the latest result that was handed out (including "nothing to show").
    settled: Option<HeatKey>,
}

impl HeatJobs {
    /// Whether `key` is what the displayed heatmap was computed from.
    pub fn is_settled(&self, key: &HeatKey) -> bool {
        self.settled.as_ref() == Some(key)
    }
}

impl Survey {
    /// Progress `0..1` of the running heatmap job, if any.
    pub fn heat_progress(&self) -> Option<f32> {
        self.heat_jobs.running.as_ref().map(|r| r.job.progress())
    }

    /// Grid layout for the current zone, `None` without a floor plan.
    pub(super) fn heat_spec(&self) -> Option<GridSpec> {
        let map = self.zone().map.as_ref()?;
        let (w, h) = (f64::from(map.width), f64::from(map.height));
        let long = w.max(h);
        Some(GridSpec { width_px: w, height_px: h, cell_px: long / HEATMAP_LONG_SIDE_CELLS, extend_px: self.influence_radius_px() })
    }

    /// Drives the background render. Returns `Some(result)` once per finished computation
    /// (`Some(None)` = nothing to show for these inputs) and `None` while idle, waiting or
    /// when the displayed heatmap is already current.
    pub fn advance_heat(&mut self) -> Option<Option<HeatOutput>> {
        let key = self.heat_key();
        if self.heat_jobs.is_settled(&key) {
            return None;
        }
        if let Some(running) = &mut self.heat_jobs.running {
            if running.key == key {
                return match running.job.poll() {
                    JobPoll::Pending => None,
                    JobPoll::Done(out) => {
                        let out = out.map(|o| self.finish_output(o));
                        Some(self.settle(key, out))
                    }
                    JobPoll::Cancelled => None,
                    JobPoll::Failed => {
                        self.error = Some("heatmap computation failed".to_owned());
                        Some(self.settle(key, None))
                    }
                };
            }
            // Inputs changed since this job started: its result would be stale.
            self.heat_jobs.running = None;
        }
        let Some(spec) = self.heat_spec() else { return Some(self.settle(key, None)) };
        let metric = self.metric();
        let samples = samples(&self.filtered_snapshot(), &metric);
        if samples.is_empty() {
            return Some(self.settle(key, None));
        }
        let (smoothing, alpha, preset, range) = (self.smoothing, self.opacity, self.gradient, self.active_range());
        let job = Job::spawn("heatmap", move |ctl: &Control| {
            let Some(grid) = rasterize_samples(&samples, &metric, &spec, smoothing, ctl)? else { return Ok(None) };
            let rgba = colorize_ranged(&metric, &grid, alpha, preset, range);
            Ok(Some(HeatOutput { grid, rgba, alpha }))
        });
        match job {
            Ok(job) => {
                self.heat_jobs.running = Some(Running { key, job });
                None
            }
            Err(e) => {
                self.error = Some(format!("could not start heatmap computation: {e}"));
                Some(self.settle(key, None))
            }
        }
    }

    /// Applies the canvas-only post-processing to a finished job: the crop area and the
    /// gray-out of unmeasured cells, both of which depend on UI settings, not on the job's
    /// inputs.
    fn finish_output(&self, mut out: HeatOutput) -> HeatOutput {
        self.crop_interpolated(&mut out.grid);
        out.rgba = self.heat_rgba(&out.grid);
        out.alpha = self.opacity;
        out
    }

    fn settle(&mut self, key: HeatKey, out: Option<HeatOutput>) -> Option<HeatOutput> {
        self.heat_jobs.running = None;
        self.heat_jobs.settled = Some(key);
        out
    }

    /// Forgets the displayed result so the next frame recomputes (after a texture reset).
    pub(super) fn invalidate_heat(&mut self) {
        self.heat_jobs = HeatJobs::default();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::survey::MetricChoice;
    use openspot_wlan::{AccessPoint, MacAddress};

    fn ap(last: u8, rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
    }

    fn survey() -> Survey {
        let mut s = Survey::default();
        s.create_blank_map(20.0, 12.0).unwrap();
        for (x, y, r) in [(100.0, 100.0, -45), (1500.0, 120.0, -70), (800.0, 700.0, -60), (200.0, 800.0, -85)] {
            s.add_scan(x, y, vec![ap(1, r), ap(2, r - 10)]);
        }
        s
    }

    fn wait_for_result(s: &mut Survey) -> Option<HeatOutput> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(out) = s.advance_heat() {
                return out;
            }
            assert!(Instant::now() < deadline, "heatmap job never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn background_result_equals_the_synchronous_path() {
        for choice in [MetricChoice::BestSignal, MetricChoice::ApCount, MetricChoice::BandCoverage] {
            let mut s = survey();
            s.metric_choice = choice;
            let out = wait_for_result(&mut s).expect("heatmap");
            let grid = s.compute_grid().expect("sync grid");
            assert_eq!(out.grid, grid, "{choice:?}");
            assert_eq!(out.rgba, colorize_ranged(&s.metric(), &grid, s.opacity, s.gradient, s.active_range()));
        }
    }

    #[test]
    fn recomputes_only_when_an_input_changes_and_cancels_the_stale_job() {
        let mut s = survey();
        assert!(wait_for_result(&mut s).is_some());
        assert!(s.advance_heat().is_none(), "unchanged inputs: nothing to do");
        assert!(s.heat_progress().is_none());
        // Change inputs twice before polling: only the latest result may come out.
        s.add_scan(400.0, 400.0, vec![ap(1, -30)]);
        assert!(s.advance_heat().is_none(), "job started, not finished instantly");
        s.add_scan(900.0, 300.0, vec![ap(1, -95)]);
        let out = wait_for_result(&mut s).expect("heatmap");
        assert_eq!(out.grid, s.compute_grid().unwrap(), "result reflects the newest data");
        assert!(s.heat_jobs.running.is_none(), "no job left behind");
    }

    #[test]
    fn nothing_to_show_settles_immediately() {
        let mut s = Survey::default();
        assert_eq!(s.advance_heat(), Some(None), "no floor plan");
        assert!(s.advance_heat().is_none(), "settled");
        s.create_blank_map(5.0, 5.0).unwrap();
        assert_eq!(s.advance_heat(), Some(None), "no measurements");
    }

    #[test]
    fn dropping_the_survey_joins_the_worker() {
        let mut s = survey();
        assert!(s.advance_heat().is_none());
        assert!(s.heat_jobs.running.is_some());
        drop(s); // Job::drop cancels and joins; returning at all proves no hang or leak
    }
}
