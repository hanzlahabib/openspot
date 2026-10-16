//! Background prediction (P-10): started when the inputs changed, cancelled when they change
//! again or on request, polled once per frame. Same pattern as Survey's heatmap job, on the
//! shared `openspot_heatmap::Job`.

use openspot_heatmap::{Job, JobPoll};
use openspot_model::{Project, Structure, Uuid};

use super::predict_input::{InputError, Prediction, PredictInput, PredictSettings, compute_in_job};

/// Everything a prediction depends on that is not in the settings: if any of it changes the
/// shown result is stale. Planning data is covered by the generation counter, which every
/// edit bumps; the rest can change from Survey mode, so it is compared by value.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictKey {
    generation: u64,
    project: Uuid,
    zone: Uuid,
    structure: Structure,
    zones: Vec<ZoneStamp>,
    settings: PredictSettings,
}

#[derive(Debug, Clone, PartialEq)]
struct ZoneStamp {
    id: Uuid,
    /// Size of the plan image data and its pixel size: any replaced plan differs.
    map: Option<(usize, u32, u32)>,
    pixels_per_meter: Option<f64>,
}

impl PredictKey {
    pub fn new(project: &Project, zone_index: usize, generation: u64, settings: &PredictSettings) -> Option<Self> {
        let zone = project.zones.get(zone_index)?;
        let stamp = |z: &openspot_model::Zone| ZoneStamp {
            id: z.id,
            map: z.map.as_ref().map(|m| (m.data.len(), m.width, m.height)),
            pixels_per_meter: z.scale.map(|s| s.pixels_per_meter()),
        };
        Some(Self {
            generation,
            project: project.id,
            zone: zone.id,
            structure: project.structure.clone(),
            zones: project.zones.iter().map(stamp).collect(),
            settings: *settings,
        })
    }
}

type JobResult = Result<Option<Prediction>, String>;

#[derive(Debug)]
struct Running {
    key: PredictKey,
    job: Job<JobResult>,
}

/// Something finished: a prediction (or "nothing to show"), or why there is none.
#[derive(Debug, PartialEq)]
pub struct Finished {
    pub result: JobResult,
}

#[derive(Debug, Default)]
pub struct PredictJobs {
    running: Option<Running>,
    /// Inputs of the latest finished result (including failures and "nothing to show").
    settled: Option<PredictKey>,
    /// Inputs the user cancelled; not restarted automatically until something changes.
    cancelled: Option<PredictKey>,
}

impl PredictJobs {
    /// Progress `0..1` of the running prediction, if any.
    pub fn progress(&self) -> Option<f32> {
        self.running.as_ref().map(|r| r.job.progress())
    }

    /// Whether the shown result was computed from `key`.
    pub fn is_settled(&self, key: &PredictKey) -> bool {
        self.settled.as_ref() == Some(key)
    }

    /// Whether the user stopped the computation for exactly these inputs.
    pub fn is_cancelled(&self, key: &PredictKey) -> bool {
        self.cancelled.as_ref() == Some(key)
    }

    /// Stops the running computation; it will not restart until the inputs change or
    /// [`advance`](Self::advance) is called with `force`.
    pub fn cancel(&mut self) {
        if let Some(r) = self.running.take() {
            // Dropping the job cancels it and waits for the worker (at most one row of work).
            self.cancelled = Some(r.key);
        }
    }

    /// Drives the job. Returns `Some` once per finished computation. `wanted` is whether a
    /// computation should start for `key` if none is current (auto mode or an explicit request);
    /// `force` also overrides an earlier cancellation. `build` is only called to start one.
    pub fn advance(&mut self, key: &PredictKey, wanted: bool, force: bool, build: impl FnOnce() -> Result<PredictInput, InputError>) -> Option<Finished> {
        if self.is_settled(key) && !force {
            return None;
        }
        if let Some(running) = &mut self.running {
            if running.key == *key {
                let result = match running.job.poll() {
                    JobPoll::Pending => return None,
                    JobPoll::Done(r) => r,
                    JobPoll::Cancelled => {
                        self.running = None;
                        return None;
                    }
                    JobPoll::Failed => Err("the prediction stopped unexpectedly".to_owned()),
                };
                return Some(self.settle(key, result));
            }
            // Inputs changed since this job started: its result would be stale.
            self.running = None;
        }
        if !wanted && !force {
            return None;
        }
        if force {
            self.cancelled = None;
        } else if self.is_cancelled(key) {
            return None;
        }
        let input = match build() {
            Ok(input) => input,
            Err(e) => return Some(self.settle(key, Err(e.to_string()))),
        };
        match Job::spawn("planning-predict", move |ctl| compute_in_job(&input, ctl)) {
            Ok(job) => {
                self.running = Some(Running { key: key.clone(), job });
                None
            }
            Err(e) => Some(self.settle(key, Err(format!("could not start the prediction: {e}")))),
        }
    }

    fn settle(&mut self, key: &PredictKey, result: JobResult) -> Finished {
        self.running = None;
        self.settled = Some(key.clone());
        Finished { result }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::planning::predict_input::tests::two_room_project;
    use crate::planning::predict_input::{build_input, compute};
    use openspot_model::MaterialCatalog;
    use openspot_predict::Control;

    fn key(p: &Project, generation: u64) -> PredictKey {
        PredictKey::new(p, 0, generation, &PredictSettings::default()).unwrap()
    }

    fn build(p: &Project) -> impl FnOnce() -> Result<PredictInput, InputError> + '_ {
        move || build_input(p, 0, &MaterialCatalog::builtin().unwrap(), &PredictSettings::default())
    }

    fn wait(jobs: &mut PredictJobs, p: &Project, k: &PredictKey) -> Finished {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(f) = jobs.advance(k, true, false, build(p)) {
                return f;
            }
            assert!(Instant::now() < deadline, "prediction never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn background_result_equals_the_synchronous_engine_result() {
        let p = two_room_project("concrete-wall");
        let k = key(&p, 1);
        let mut jobs = PredictJobs::default();
        let finished = wait(&mut jobs, &p, &k);
        let sync = compute(&build(&p)().unwrap(), &Control::default()).unwrap();
        assert_eq!(finished.result, Ok(sync));
        assert!(finished.result.as_ref().unwrap().is_some());
        assert!(jobs.is_settled(&k) && jobs.progress().is_none());
        assert!(jobs.advance(&k, true, false, build(&p)).is_none(), "settled inputs start nothing");
    }

    #[test]
    fn changed_inputs_discard_the_running_job_and_recompute() {
        let mut p = two_room_project("drywall");
        let mut jobs = PredictJobs::default();
        let k1 = key(&p, 1);
        assert!(jobs.advance(&k1, true, false, build(&p)).is_none(), "started, not finished instantly");
        assert!(jobs.progress().is_some());
        // The plan is edited before the first job finished.
        p.zones[0].planning.walls[0].material = "concrete-wall".into();
        let k2 = key(&p, 2);
        let f = wait(&mut jobs, &p, &k2);
                let sync = compute(&build(&p)().unwrap(), &Control::default()).unwrap();
        assert_eq!(f.result, Ok(sync), "and it reflects the edit");
    }

    #[test]
    fn manual_mode_waits_for_a_request() {
        let p = two_room_project("drywall");
        let k = key(&p, 1);
        let mut jobs = PredictJobs::default();
        assert!(jobs.advance(&k, false, false, build(&p)).is_none());
        assert!(jobs.progress().is_none(), "auto off: nothing starts by itself");
        assert!(jobs.advance(&k, false, true, build(&p)).is_none());
        assert!(jobs.progress().is_some(), "an explicit request starts it");
    }

    #[test]
    fn cancelled_inputs_stay_cancelled_until_they_change_or_are_forced() {
        let p = two_room_project("drywall");
        let k = key(&p, 1);
        let mut jobs = PredictJobs::default();
        assert!(jobs.advance(&k, true, false, build(&p)).is_none());
        jobs.cancel();
        assert!(jobs.progress().is_none() && jobs.is_cancelled(&k) && !jobs.is_settled(&k));
        assert!(jobs.advance(&k, true, false, build(&p)).is_none() && jobs.progress().is_none(), "auto mode does not restart a cancelled run");
        assert!(jobs.advance(&key(&p, 2), true, false, build(&p)).is_none() && jobs.progress().is_some(), "an edit starts a fresh run");
        jobs.cancel();
        assert!(jobs.advance(&key(&p, 2), true, true, build(&p)).is_none() && jobs.progress().is_some(), "force overrides the cancel");
    }

    #[test]
    fn input_errors_settle_once_instead_of_retrying_every_frame() {
        let mut p = two_room_project("drywall");
        p.zones[0].scale = None;
        let k = key(&p, 1);
        let mut jobs = PredictJobs::default();
        let f = jobs.advance(&k, true, false, build(&p)).expect("failure is reported at once");
        assert!(f.result.unwrap_err().contains("scale"));
        assert!(jobs.advance(&k, true, false, build(&p)).is_none(), "and not again");
    }

    #[test]
    fn keys_notice_outside_changes() {
        let mut p = two_room_project("drywall");
        let k = key(&p, 1);
        assert_eq!(k, key(&p, 1));
        assert_ne!(k, key(&p, 2), "an edit");
        p.zones[0].scale = openspot_model::Scale::new(40.0);
        assert_ne!(k, key(&p, 1), "a rescale in Survey mode");
        let mut regrouped = p.clone();
        regrouped.structure.add_building("B");
        assert_ne!(key(&p, 1), key(&regrouped, 1), "a structure change");
        let slower = PredictSettings { detail: 0, ..PredictSettings::default() };
        assert_ne!(k, PredictKey::new(&p, 0, 1, &slower).unwrap());
        assert!(PredictKey::new(&p, 5, 1, &slower).is_none());
    }

    #[test]
    fn dropping_jobs_joins_the_worker() {
        let p = two_room_project("drywall");
        let mut jobs = PredictJobs::default();
        assert!(jobs.advance(&key(&p, 1), true, false, build(&p)).is_none());
        assert!(jobs.progress().is_some());
        drop(jobs); // Job::drop cancels and joins; returning at all proves no hang or leak
    }
}
