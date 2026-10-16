//! Planning session state for results: the shown prediction, the calibration window and the
//! access-point suggestions, with the `Planning` methods that drive them.

use std::collections::BTreeSet;

use openspot_model::{Project, Uuid};
use openspot_predict::PropagationModel;
use openspot_wlan::MacAddress;

use super::edit::{Change, Edit};
use super::ops::Selection;
use super::predict_input::{Prediction, build_input};
use super::predict_job::PredictKey;
use super::results::{ViewImage, ViewSettings};
use super::{Planning, ap_models, calibrate_fit, results, suggest};

/// The "calibrate to survey" window and its latest fit.
#[derive(Debug, Default)]
pub struct CalibrationState {
    pub open: bool,
    /// Snapshot of the shown zone to compare with.
    pub snapshot: usize,
    /// Networks that count as the planned system; empty means every network heard.
    pub networks: BTreeSet<MacAddress>,
    pub outcome: Option<Result<calibrate_fit::Report, String>>,
}

/// The "suggest AP positions" window, its running computation and the proposals.
#[derive(Debug, Default)]
pub struct SuggestState {
    pub open: bool,
    pub params: suggest::SuggestParams,
    pub(super) job: suggest::SuggestJob,
    /// Zone the proposals were computed for, and the plan generation they belong to.
    pub(super) zone: Option<Uuid>,
    pub(super) for_generation: u64,
    pub results: Option<Result<Vec<suggest::Proposal>, String>>,
}

/// A finished prediction.
#[derive(Debug)]
pub struct Shown {
    /// `None`: the plan has no enabled radio, so there is nothing to show.
    pub prediction: Option<Prediction>,
    /// Distinguishes results, so caches know when the prediction changed.
    pub(super) serial: u64,
}

/// The rendered view of the shown prediction, with its GPU texture.
pub(super) struct HeatCache {
    pub(super) serial: u64,
    pub(super) settings: ViewSettings,
    pub(super) image: Option<ViewImage>,
    pub(super) texture: Option<eframe::egui::TextureHandle>,
}


impl Planning {
    // ---- prediction ------------------------------------------------------------------

    /// Starts, polls or restarts the background prediction for the shown zone. Call every
    /// frame. `force` starts one now even when auto-update is off or was cancelled.
    pub fn advance_prediction(&mut self, project: &Project, zone: usize, force: bool) {
        let Some(key) = PredictKey::new(project, zone, self.generation, &self.predict_settings) else { return };
        let (catalog, settings) = (&self.catalog, self.predict_settings);
        let finished = self.jobs.advance(&key, self.auto_predict, force, || build_input(project, zone, catalog, &settings));
        let Some(done) = finished else { return };
        match done.result {
            Ok(prediction) => {
                self.shown_serial += 1;
                self.shown = Some(Shown { prediction, serial: self.shown_serial });
                self.predict_error = None;
            }
            Err(e) => {
                self.shown = None;
                self.predict_error = Some(e);
            }
        }
    }

    // ---- calibration -----------------------------------------------------------------

    /// Fits the propagation model to the chosen snapshot and keeps the report for display.
    pub fn run_calibration(&mut self, project: &Project, zone: usize) {
        let c = &self.calibration;
        let outcome = calibrate_fit::fit(project, zone, c.snapshot, &c.networks, &self.catalog, &self.predict_settings.model, self.predict_settings.rx_height_m);
        self.calibration.outcome = Some(outcome.map_err(|e| e.to_string()));
    }

    /// Uses the last fit for predictions (they recompute on their own).
    pub fn apply_calibration(&mut self) {
        if let Some(Ok(report)) = &self.calibration.outcome {
            self.predict_settings.model = report.fit.model();
            self.notice = Some(format!("using exponent {:.2}, offset {:+.1} dB", report.fit.exponent, report.fit.offset_db));
        }
    }

    /// Back to the default, uncalibrated model.
    pub fn reset_calibration(&mut self) {
        self.predict_settings.model = PropagationModel::default();
        self.calibration.outcome = None;
    }

    // ---- AP suggestions --------------------------------------------------------------

    /// Starts computing proposals for the shown zone, replacing earlier ones.
    pub fn start_suggestions(&mut self, project: &Project, zone: usize) {
        self.suggest.results = None;
        let input = suggest::build_input(project, zone, &self.catalog, self.predict_settings.model, self.predict_settings.rx_height_m, &self.settings.ap_model, &self.suggest.params);
        let started = input.map_err(|e| e.to_string()).and_then(|input| self.suggest.job.start(input));
        match started {
            Ok(()) => self.suggest.zone = project.zones.get(zone).map(|z| z.id),
            Err(e) => self.suggest.results = Some(Err(e)),
        }
    }

    /// Collects a finished computation. Call every frame.
    pub fn poll_suggestions(&mut self) {
        if let Some(result) = self.suggest.job.poll() {
            self.suggest.for_generation = self.generation;
            self.suggest.results = Some(result.map(|list| suggest::proposals(&list)));
        }
    }

    pub fn suggestions_progress(&self) -> Option<f32> {
        self.suggest.job.progress()
    }

    pub fn cancel_suggestions(&mut self) {
        self.suggest.job.cancel();
    }

    pub fn clear_suggestions(&mut self) {
        self.suggest.results = None;
    }

    /// Proposals for `zone`, if they were computed for it.
    pub fn suggestions_for(&self, zone: Uuid) -> &[suggest::Proposal] {
        match (&self.suggest.results, self.suggest.zone) {
            (Some(Ok(s)), Some(z)) if z == zone => s,
            _ => &[],
        }
    }

    /// Whether the proposals were computed from the plan as it is now.
    pub fn suggestions_are_current(&self) -> bool {
        self.suggest.for_generation == self.generation
    }

    /// Turns proposal `index` into a real access point using the model selected for new APs.
    pub fn accept_suggestion(&mut self, project: &mut Project, zone: usize, index: usize) -> bool {
        let Some(Ok(list)) = &mut self.suggest.results else { return false };
        if index >= list.len() {
            return false;
        }
        let Some(z) = project.zones.get(zone) else { return false };
        if self.suggest.zone != Some(z.id) {
            return false;
        }
        let picked = list.remove(index);
        let ap = self.settings.ap_model.instantiate(ap_models::next_ap_name(&z.planning), picked.position);
        let (id, at) = (ap.id, z.planning.access_points.len());
        let committed = self.commit(project, Edit::Ap { zone: z.id, change: Change::added(at, ap) });
        // Our own edit changed the plan; the remaining proposals are still the engine's answer.
        self.suggest.for_generation = self.generation;
        self.editor.selection = Some(Selection::Ap(id));
        committed
    }

    /// Accepts every remaining proposal, in order.
    pub fn accept_all_suggestions(&mut self, project: &mut Project, zone: usize) {
        while matches!(&self.suggest.results, Some(Ok(l)) if !l.is_empty()) {
            if !self.accept_suggestion(project, zone, 0) {
                break;
            }
        }
    }

    pub fn cancel_prediction(&mut self) {
        self.jobs.cancel();
    }

    /// Whether the user stopped the computation for the plan as it is now.
    pub fn prediction_stopped(&self, project: &Project, zone: usize) -> bool {
        PredictKey::new(project, zone, self.generation, &self.predict_settings).is_some_and(|k| self.jobs.is_cancelled(&k))
    }

    pub fn prediction_progress(&self) -> Option<f32> {
        self.jobs.progress()
    }

    /// The prediction on show, if any (it may be stale while a newer one is computed).
    pub fn shown(&self) -> Option<&Shown> {
        self.shown.as_ref()
    }

    /// Whether the shown prediction was computed from the current plan.
    pub fn shown_is_current(&self, project: &Project, zone: usize) -> bool {
        PredictKey::new(project, zone, self.generation, &self.predict_settings).is_some_and(|k| self.jobs.is_settled(&k))
    }

    /// The view of the shown prediction for the current view settings, rebuilt only when
    /// either changed.
    pub fn heat_image(&mut self) -> Option<&ViewImage> {
        let serial = self.shown.as_ref().map(|s| s.serial)?;
        let settings = self.view_settings;
        let stale = self.heat.as_ref().is_none_or(|h| h.serial != serial || h.settings != settings);
        if stale {
            let image = self.shown.as_ref().and_then(|s| s.prediction.as_ref()).and_then(|p| results::render(p, &settings));
            self.heat = Some(HeatCache { serial, settings, image, texture: None });
        }
        self.heat.as_ref().and_then(|h| h.image.as_ref())
    }

    /// PNG of the plan with the shown view blended over it and the access points marked.
    pub fn export_png(&mut self, project: &Project, zone: usize) -> Result<Vec<u8>, String> {
        let z = project.zones.get(zone).ok_or("no zone")?;
        let scale = z.scale.ok_or("set the plan's scale first")?;
        let markers: Vec<_> = z
            .planning
            .access_points
            .iter()
            .map(|a| openspot_export::Marker { x: scale.to_pixels(a.position.x), y: scale.to_pixels(a.position.y), rgb: [30, 110, 235] })
            .collect();
        let image = self.heat_image().ok_or("there is no prediction to export yet")?;
        openspot_export::render_png_raster(z, image.grid.cols as u32, image.grid.rows as u32, image.rgba.clone(), image.grid.cell_px, &markers).map_err(|e| e.to_string())
    }
}
