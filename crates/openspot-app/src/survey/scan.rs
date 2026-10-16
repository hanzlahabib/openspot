//! Scan requests and results: the single path from "measure here" to a recorded point.

use openspot_model::{LinkInfo, MAX_SAMPLES_PER_POINT, RadiusPreset, SpeedResult};
use openspot_wlan::AccessPoint;

use super::Survey;
use super::cue::{ScanOutcome, cue_for};
use crate::sound::Cue;
use super::sampling::Sampler;
use crate::worker::{Command, Worker};

impl Survey {
    pub fn radius_preset(&self) -> Option<RadiusPreset> {
        self.project.canvas.zone(self.zone().id).and_then(|z| z.radius)
    }

    pub fn samples_per_point(&self) -> u8 {
        self.project.canvas.zone(self.zone().id).map_or(1, |z| z.samples_per_point)
    }

    pub fn set_radius_preset(&mut self, preset: Option<RadiusPreset>) {
        if preset != self.radius_preset() {
            let id = self.zone().id;
            self.project.canvas.zone_mut(id).radius = preset;
            self.touch();
        }
    }

    pub fn set_samples_per_point(&mut self, n: u8) {
        let n = n.clamp(1, MAX_SAMPLES_PER_POINT);
        if n != self.samples_per_point() {
            let id = self.zone().id;
            self.project.canvas.zone_mut(id).samples_per_point = n;
            self.touch();
        }
    }

    /// Remembers the adapter's connection reported with the scan that is about to be
    /// handled by [`Survey::on_scan_result`]. With several scans per point the latest one
    /// counts, like the speed results.
    pub fn set_scan_link(&mut self, link: Option<LinkInfo>) {
        self.scan_link = link;
    }

    /// Asks for a measurement at map position `(x, y)`; the UI layer forwards the scans
    /// it needs to the worker.
    pub fn request_scan(&mut self, x: f64, y: f64) {
        self.busy = true;
        self.error = None;
        self.scan_link = None;
        self.sampler = Some(Sampler::new(x, y, self.samples_per_point()));
        self.pending_scan = Some((x, y));
    }

    /// Takes the scan the UI should send to the worker, if one was requested.
    pub fn take_pending_scan(&mut self) -> Option<(f64, f64)> {
        self.pending_scan.take()
    }

    /// Sends the pending scan for `(x, y)` to the worker. Active (speed) tests run once per
    /// point, together with the last passive scan; earlier samples are passive only.
    pub(crate) fn dispatch_scan(&mut self, worker: &Worker, x: f64, y: f64) {
        let is_last = self.sampler.as_ref().is_none_or(|s| {
            let (done, needed) = s.progress();
            done + 1 >= needed
        });
        let command = if is_last {
            match self.active.command_for(x, y) {
                Ok(command) => command,
                Err(message) => {
                    self.busy = false;
                    self.sampler = None;
                    self.error = Some(message);
                    self.path_scan_finished(false);
                    return;
                }
            }
        } else {
            Command::SurveyScan { x, y }
        };
        worker.send(command);
    }

    /// Handles the worker's answer for a scan at `(x, y)`. With several scans per point
    /// the next one is requested until enough are collected, then their average is
    /// recorded as one point (with the speed results of the final scan). A failed scan
    /// discards the partial point.
    pub fn on_scan_result(&mut self, x: f64, y: f64, result: Result<Vec<AccessPoint>, String>, speed: Vec<SpeedResult>) {
        self.active.progress = None;
        let mut sampler = self.sampler.take().unwrap_or_else(|| Sampler::new(x, y, 1));
        match result {
            Ok(aps) => {
                if sampler.push(aps) {
                    self.busy = false;
                    let (px, py) = (sampler.x, sampler.y);
                    let failures = crate::active_scan::summarize_failures(&speed);
                    self.add_scan_with_speed(px, py, sampler.finish(), speed);
                    self.error = failures;
                    self.message = Some("Point recorded".to_owned());
                    self.pending_cue = cue_for(self.sound_enabled, ScanOutcome::Recorded, self.path_run.is_some());
                    self.path_scan_finished(true);
                } else {
                    let (done, needed) = sampler.progress();
                    self.message = Some(format!("Scan {done} of {needed} done, stand still…"));
                    self.pending_scan = Some((sampler.x, sampler.y));
                    self.sampler = Some(sampler);
                }
            }
            Err(e) => {
                self.busy = false;
                self.error = Some(e);
                self.scan_link = None;
                self.pending_cue = cue_for(self.sound_enabled, ScanOutcome::Failed, self.path_run.is_some());
                self.path_scan_finished(false);
            }
        }
    }

    /// The sound the last finished measurement asks for, once.
    pub fn take_cue(&mut self) -> Option<Cue> {
        self.pending_cue.take()
    }
}
