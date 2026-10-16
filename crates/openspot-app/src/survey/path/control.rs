//! Survey-level path controls: editing the route, starting/pausing runs, feeding scans.

use openspot_model::{SurveyPath, Uuid};

use super::{PathError, PathPlan, PathRun, Point, RunState};
use crate::survey::Survey;

/// A run bound to the snapshot it records into, so switching snapshots cannot misfile points.
#[derive(Debug)]
pub struct ActiveRun {
    pub run: PathRun,
    pub snapshot: Uuid,
}

impl Survey {
    pub fn path(&self) -> Option<&SurveyPath> {
        self.project.canvas.snapshot(self.snapshot().id).map(|c| &c.path)
    }

    pub fn path_waypoints(&self) -> &[Point] {
        self.path().map_or(&[], |p| p.waypoints.as_slice())
    }

    pub fn path_interval_m(&self) -> f64 {
        self.path().map_or(openspot_model::DEFAULT_PATH_INTERVAL_M, |p| p.interval_m)
    }

    pub fn path_running(&self) -> bool {
        self.path_run.as_ref().is_some_and(|a| a.run.is_active())
    }

    fn path_mut(&mut self) -> &mut SurveyPath {
        let id = self.snapshot().id;
        &mut self.project.canvas.snapshot_mut(id).path
    }

    /// Adds a waypoint. Ignored while a run is active (the route is being walked).
    pub fn path_click(&mut self, x: f64, y: f64) -> bool {
        if self.path_running() {
            return false;
        }
        self.path_mut().waypoints.push((x, y));
        self.touch();
        true
    }

    pub fn path_remove_last(&mut self) -> bool {
        if self.path_running() || self.path_waypoints().is_empty() {
            return false;
        }
        self.path_mut().waypoints.pop();
        self.touch();
        true
    }

    pub fn path_clear(&mut self) -> bool {
        if self.path_running() || self.path_waypoints().is_empty() {
            return false;
        }
        self.path_mut().waypoints.clear();
        self.touch();
        true
    }

    pub fn set_path_interval_m(&mut self, meters: f64) {
        if meters.is_finite() && meters > 0.0 && meters != self.path_interval_m() {
            self.path_mut().interval_m = meters;
            self.touch();
        }
    }

    /// Starts sampling along the stored route. Any warning about the plan (no scale) is
    /// shown as the survey message.
    pub fn path_start(&mut self, now: f64) -> Result<(), PathError> {
        let plan = PathPlan::new(self.path_waypoints(), self.zone().scale, self.path_interval_m(), self.walk_speed_mps)?;
        self.message = plan.warning.clone();
        self.error = None;
        self.path_run = Some(ActiveRun { run: PathRun::start(plan, now), snapshot: self.snapshot().id });
        Ok(())
    }

    pub fn path_pause(&mut self, now: f64) {
        if let Some(a) = &mut self.path_run {
            a.run.pause(now);
        }
    }

    pub fn path_resume(&mut self, now: f64) {
        if let Some(a) = &mut self.path_run {
            a.run.resume(now);
        }
    }

    pub fn path_cancel(&mut self) {
        if let Some(a) = self.path_run.take() {
            let (done, total) = a.run.progress();
            self.message = Some(format!("Path run cancelled after {done} of {total} points"));
        }
    }

    /// Advances the run; call every frame with a monotonic clock in seconds. Requests the
    /// next scan when one is due.
    pub fn drive_path(&mut self, now: f64) {
        let Some(active) = &self.path_run else { return };
        if active.snapshot != self.snapshot().id {
            self.path_cancel();
            return;
        }
        // A manual scan owns the radio; the run clock keeps ticking and catches up after.
        if self.busy {
            return;
        }
        let Some(due) = self.path_run.as_mut().and_then(|a| a.run.poll(now)) else { return };
        self.request_scan(due.x, due.y);
    }

    /// Reports a finished scan to the run (if any).
    pub(crate) fn path_scan_finished(&mut self, ok: bool) {
        let Some(active) = &mut self.path_run else { return };
        active.run.scan_finished(ok);
        match active.run.state() {
            RunState::Finished => {
                let total = active.run.progress().1;
                self.path_run = None;
                self.message = Some(format!("Path complete: {total} points recorded"));
            }
            RunState::Paused if !ok => {
                self.message = Some("Path paused because a scan failed; resume to retry".to_owned());
            }
            _ => {}
        }
    }
}
