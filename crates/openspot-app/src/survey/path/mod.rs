//! Survey path mode: sample positions along a drawn route and schedule scans on them.
//!
//! The walker follows the route at a constant pace, so the sample at arc length `d` is due
//! `d / speed` seconds after the run started. Everything here is pure (positions and a
//! clock value in, decisions out); the egui layer only forwards clicks and `now`.

mod control;

use openspot_model::Scale;

pub use control::ActiveRun;
use thiserror::Error;

pub type Point = (f64, f64);

/// Relative slack when deciding whether the route length is an exact multiple of the
/// step, so floating-point error does not add a second, almost identical end sample.
const END_TOLERANCE: f64 = 1e-9;
/// Upper bound on samples in one run. A walk this long is a mistake (interval typed in
/// the wrong unit), and it keeps the plan's memory and the run time bounded.
pub const MAX_PATH_SAMPLES: usize = 2000;
/// Default walking pace while surveying: slower than a stroll so the person can stand
/// still for the scan at each sample.
pub const DEFAULT_WALK_SPEED_MPS: f64 = 0.8;
/// Sample spacing when the zone has no scale and the interval cannot be converted. About
/// a thirtieth of a typical 1600 px plan, so the route still gets a usable number of points.
pub const FALLBACK_INTERVAL_PX: f64 = 60.0;
/// Seconds between samples without a scale (no pace can be derived from pixels).
pub const FALLBACK_STEP_SECS: f64 = 3.0;

/// Positions at arc-length `0, step, 2*step, ...` along the polyline through `waypoints`,
/// plus the final waypoint (unless the last regular sample already lands on it).
///
/// Arc length runs continuously around corners, so spacing along the route stays `step`
/// even where it bends (corner vertices are not sampled unless they fall on a multiple).
/// Repeated waypoints and zero-length segments are ignored. A route with no length yields
/// its single point; an empty route or a non-positive/non-finite step yields nothing.
pub fn sample_along(waypoints: &[Point], step: f64) -> Vec<Point> {
    if !(step.is_finite() && step > 0.0) {
        return Vec::new();
    }
    let mut pts: Vec<Point> = Vec::with_capacity(waypoints.len());
    for &p in waypoints.iter().filter(|p| p.0.is_finite() && p.1.is_finite()) {
        if pts.last() != Some(&p) {
            pts.push(p);
        }
    }
    let Some(&first) = pts.first() else { return Vec::new() };
    // cumulative[i] = arc length at pts[i]
    let mut cumulative = vec![0.0];
    for w in pts.windows(2) {
        let prev = cumulative.last().copied().unwrap_or(0.0);
        cumulative.push(prev + (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1));
    }
    let total = cumulative.last().copied().unwrap_or(0.0);
    if total <= 0.0 {
        return vec![first];
    }
    let at = |d: f64| -> Point {
        let d = d.clamp(0.0, total);
        // First vertex whose cumulative length reaches d; the segment ends there.
        let hi = cumulative.partition_point(|&c| c < d).clamp(1, pts.len() - 1);
        let (a, b) = (pts[hi - 1], pts[hi]);
        let seg = cumulative[hi] - cumulative[hi - 1];
        let t = if seg > 0.0 { (d - cumulative[hi - 1]) / seg } else { 0.0 };
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    };
    let regular = (total / step * (1.0 + END_TOLERANCE)).floor() as usize;
    let mut out: Vec<Point> = (0..=regular).map(|k| at(k as f64 * step)).collect();
    let covered = regular as f64 * step;
    if total - covered > total * END_TOLERANCE {
        out.push(pts[pts.len() - 1]);
    }
    out
}

/// Total length of the polyline.
pub fn route_length(waypoints: &[Point]) -> f64 {
    waypoints.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum()
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathError {
    #[error("click at least two waypoints to define a route")]
    TooShort,
    #[error("the sampling interval and walking speed must be positive")]
    BadParameters,
    #[error("this route would need more than {MAX_PATH_SAMPLES} samples; use a larger interval")]
    TooManySamples,
}

/// Where and when to measure along a route.
#[derive(Debug, Clone, PartialEq)]
pub struct PathPlan {
    pub targets: Vec<Point>,
    /// Seconds between consecutive samples.
    pub step_secs: f64,
    /// Set when the plan had to deviate from what was asked (no scale).
    pub warning: Option<String>,
}

impl PathPlan {
    /// Plans samples every `interval_m` meters walking at `speed_mps`. Without a scale the
    /// interval cannot be converted to pixels, so [`FALLBACK_INTERVAL_PX`] and
    /// [`FALLBACK_STEP_SECS`] are used and a warning is attached.
    pub fn new(waypoints: &[Point], scale: Option<Scale>, interval_m: f64, speed_mps: f64) -> Result<Self, PathError> {
        if waypoints.len() < 2 || route_length(waypoints) <= 0.0 {
            return Err(PathError::TooShort);
        }
        if !(interval_m.is_finite() && interval_m > 0.0 && speed_mps.is_finite() && speed_mps > 0.0) {
            return Err(PathError::BadParameters);
        }
        let (step_px, step_secs, warning) = match scale {
            Some(s) => (s.to_pixels(interval_m), interval_m / speed_mps, None),
            None => (
                FALLBACK_INTERVAL_PX,
                FALLBACK_STEP_SECS,
                Some(format!("No scale set: sampling every {FALLBACK_INTERVAL_PX:.0} px instead of {interval_m} m. Calibrate the scale first for real distances.")),
            ),
        };
        if route_length(waypoints) / step_px > MAX_PATH_SAMPLES as f64 {
            return Err(PathError::TooManySamples);
        }
        Ok(Self { targets: sample_along(waypoints, step_px), step_secs, warning })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Running,
    Paused,
    Finished,
}

/// A sample the run wants measured now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DueSample {
    pub index: usize,
    pub x: f64,
    pub y: f64,
}

/// Scheduler for one path run. Time only advances while running (pauses stop the clock),
/// and at most one scan is in flight at a time; a late scan delays the following ones
/// rather than being skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct PathRun {
    plan: PathPlan,
    state: RunState,
    next: usize,
    in_flight: bool,
    /// Run time in seconds, excluding pauses.
    elapsed: f64,
    last_now: Option<f64>,
}

impl PathRun {
    pub fn start(plan: PathPlan, now: f64) -> Self {
        Self { plan, state: RunState::Running, next: 0, in_flight: false, elapsed: 0.0, last_now: Some(now) }
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state, RunState::Running | RunState::Paused)
    }

    /// `(completed samples, total samples)`.
    pub fn progress(&self) -> (usize, usize) {
        (self.next, self.plan.targets.len())
    }

    fn advance_clock(&mut self, now: f64) {
        if let Some(last) = self.last_now.replace(now) {
            self.elapsed += (now - last).max(0.0);
        }
    }

    /// Returns the sample to measure now, if one is due. Call every frame; marks the
    /// sample in flight until [`Self::scan_finished`].
    pub fn poll(&mut self, now: f64) -> Option<DueSample> {
        if self.state != RunState::Running {
            return None;
        }
        self.advance_clock(now);
        if self.in_flight || self.next >= self.plan.targets.len() {
            return None;
        }
        if self.elapsed + f64::EPSILON < self.next as f64 * self.plan.step_secs {
            return None;
        }
        self.in_flight = true;
        let (x, y) = self.plan.targets[self.next];
        Some(DueSample { index: self.next, x, y })
    }

    /// Seconds until the next sample is due (0 when due or nothing is scheduled).
    pub fn seconds_to_next(&self) -> f64 {
        (self.next as f64 * self.plan.step_secs - self.elapsed).max(0.0)
    }

    /// Reports the outcome of the sample returned by [`Self::poll`]. A failure pauses the
    /// run on the same sample so resuming retries it.
    pub fn scan_finished(&mut self, ok: bool) {
        if !self.in_flight {
            return;
        }
        self.in_flight = false;
        if !self.is_active() {
            return;
        }
        if ok {
            self.next += 1;
            if self.next >= self.plan.targets.len() {
                self.state = RunState::Finished;
            }
        } else if self.state == RunState::Running {
            self.state = RunState::Paused;
            self.last_now = None;
        }
    }

    pub fn pause(&mut self, now: f64) {
        if self.state == RunState::Running {
            self.advance_clock(now);
            self.state = RunState::Paused;
            self.last_now = None;
        }
    }

    pub fn resume(&mut self, now: f64) {
        if self.state == RunState::Paused {
            self.state = RunState::Running;
            self.last_now = Some(now);
        }
    }
}

#[cfg(test)]
mod tests;
