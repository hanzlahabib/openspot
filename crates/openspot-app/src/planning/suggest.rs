//! Suggesting access-point positions (P-09): greedy maximum coverage from the engine, run on a
//! worker thread, with the candidate lattice and grid sized so it finishes in seconds.
//!
//! This places APs for coverage only. It does no channel planning and has no interference
//! term (see `docs/PLANNING_ENGINE.md`); the UI says so.

use openspot_heatmap::{Cancelled, Control as JobControl, Job, JobPoll};
use openspot_model::{Band, MaterialCatalog, PlanningData, Point, Project, Radio};
use openspot_predict::placement::{MAX_CANDIDATES, PlacementRequest, Suggestion, candidate_lattice, suggest_placements};
use openspot_predict::{GridRequest, Mode, PropagationModel, Scene};

use super::ap_edit::new_radio;
use super::ap_models::ApModel;
use super::geometry::closest_on_segment;
use super::levels::LevelStack;
use super::predict_input::{InputError, in_job};

/// Cells along the plan's longer side in the coverage grid used for scoring candidates.
/// Cost is candidates x cells x walls, so this is much coarser than the display grid.
const SCORING_CELLS_LONG_SIDE: f64 = 60.0;
/// Smallest scoring cell: below this a wall's thickness matters more than the position.
const MIN_SCORING_CELL_M: f64 = 0.25;
/// Candidates are kept at least this far from the plan's outer edge (APs are not placed on the boundary).
pub const CANDIDATE_MARGIN_M: f64 = 0.5;
/// Candidates closer than this to a wall are dropped (an AP is not mounted inside a wall).
const WALL_CLEARANCE_M: f64 = 0.3;
pub const DEFAULT_SPACING_M: f64 = 2.0;
pub const DEFAULT_THRESHOLD_DBM: f64 = -67.0;
pub const DEFAULT_COUNT: usize = 3;
pub const MAX_COUNT: usize = 20;

/// What the user asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SuggestParams {
    pub count: usize,
    /// A cell counts as covered at or above this signal.
    pub threshold_dbm: f64,
    pub band: Band,
    /// Wanted distance between candidate positions; coarsened when it would exceed [`MAX_CANDIDATES`].
    pub spacing_m: f64,
}

impl Default for SuggestParams {
    fn default() -> Self {
        Self { count: DEFAULT_COUNT, threshold_dbm: DEFAULT_THRESHOLD_DBM, band: Band::Ghz5, spacing_m: DEFAULT_SPACING_M }
    }
}

/// Owned input for the worker.
#[derive(Debug, Clone)]
pub struct SuggestInput {
    pub stack: LevelStack,
    pub catalog: MaterialCatalog,
    pub model: PropagationModel,
    pub grid: GridRequest,
    pub candidates: Vec<Point>,
    pub template: Radio,
    pub height_m: f64,
    pub count: usize,
    pub threshold_dbm: f64,
}

/// How much the spacing grows per step while the lattice is still over the limit: 5 % lands
/// within that of the finest spacing that fits, in a handful of steps.
const SPACING_GROWTH: f64 = 1.05;

/// Candidate spacing close to `wanted_m` that keeps the lattice within [`MAX_CANDIDATES`] for a
/// plan this size. The area estimate alone undercounts (a lattice has a row and column more than
/// area / spacing squared), so the real lattice is measured.
pub fn effective_spacing_m(width_m: f64, height_m: f64, wanted_m: f64) -> f64 {
    let mut spacing = wanted_m.max((width_m * height_m / MAX_CANDIDATES as f64).sqrt()).max(0.01);
    while candidate_lattice(width_m, height_m, spacing, CANDIDATE_MARGIN_M).len() > MAX_CANDIDATES {
        spacing *= SPACING_GROWTH;
    }
    spacing
}

/// Drops candidates within [`WALL_CLEARANCE_M`] (plus half the wall's thickness) of a wall.
pub fn clear_of_walls(data: &PlanningData, candidates: Vec<Point>) -> Vec<Point> {
    candidates
        .into_iter()
        .filter(|c| data.walls.iter().all(|w| closest_on_segment(*c, w.a, w.b).distance > WALL_CLEARANCE_M + w.thickness_m / 2.0))
        .collect()
}

/// The radio new APs of `model` have in `band`, or a plain one if the model has none.
pub fn template_radio(model: &ApModel, band: Band) -> Radio {
    model.radios.iter().find(|r| r.band == band).cloned().unwrap_or_else(|| new_radio(band))
}

pub fn build_input(project: &Project, zone_index: usize, catalog: &MaterialCatalog, model: PropagationModel, rx_height_m: f64, ap_model: &ApModel, params: &SuggestParams) -> Result<SuggestInput, InputError> {
    let zone = project.zones.get(zone_index).ok_or(super::levels::StackError::NoZone)?;
    let map = zone.map.as_ref().ok_or(InputError::NoPlan)?;
    let scale = zone.scale.ok_or(InputError::NoScale)?;
    let (width_m, height_m) = (scale.to_meters(f64::from(map.width)), scale.to_meters(f64::from(map.height)));
    let stack = LevelStack::build(project, zone_index)?;
    let spacing = effective_spacing_m(width_m, height_m, params.spacing_m);
    let lattice = candidate_lattice(width_m, height_m, spacing, CANDIDATE_MARGIN_M);
    let candidates = clear_of_walls(&stack.levels[stack.active], lattice);
    let grid = GridRequest {
        level: stack.active,
        width_m,
        height_m,
        cell_m: (width_m.max(height_m) / SCORING_CELLS_LONG_SIDE).max(MIN_SCORING_CELL_M),
        rx_height_m,
        pixels_per_meter: scale.pixels_per_meter(),
        mode: Mode::Direct,
    };
    Ok(SuggestInput {
        stack,
        catalog: catalog.clone(),
        model,
        grid,
        candidates,
        template: template_radio(ap_model, params.band),
        height_m: ap_model.height_m,
        count: params.count.clamp(1, MAX_COUNT),
        threshold_dbm: params.threshold_dbm,
    })
}

/// Runs the engine.
pub fn compute(input: &SuggestInput, control: &openspot_predict::Control<'_>) -> Result<Vec<Suggestion>, openspot_predict::PredictError> {
    let refs: Vec<&PlanningData> = input.stack.levels.iter().collect();
    let scene = Scene::build(&refs, &input.catalog)?;
    let existing = Scene::transmitters(&refs)?;
    let request = PlacementRequest { candidates: &input.candidates, template: &input.template, height_m: input.height_m, count: input.count, threshold_dbm: input.threshold_dbm, grid: input.grid };
    suggest_placements(&scene, &input.model, &existing, &request, control)
}

/// [`compute`] on a worker thread.
pub fn compute_in_job(input: &SuggestInput, ctl: &JobControl<'_>) -> Result<Result<Vec<Suggestion>, String>, Cancelled> {
    let rows = match input.grid.dimensions() {
        Ok((_, rows)) => rows,
        Err(e) => return Ok(Err(e.to_string())),
    };
    let existing: usize = input.stack.levels.iter().flat_map(|l| &l.access_points).flat_map(|a| &a.radios).filter(|r| r.enabled && r.band == input.template.band).count();
    in_job(ctl, rows * (existing + input.candidates.len()), |control| compute(input, control))
}

/// One proposed access point as the UI shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Proposal {
    pub position: Point,
    /// Share of the plan this AP brings above the threshold on top of the ones before it.
    pub gain_fraction: f64,
    /// Share of the plan covered once this and the earlier proposals are in place.
    pub covered_fraction: f64,
}

/// The engine's cumulative suggestions as per-AP gains.
pub fn proposals(list: &[Suggestion]) -> Vec<Proposal> {
    let mut before = 0.0;
    list.iter()
        .map(|s| {
            let gain_fraction = (s.covered_fraction - before).max(0.0);
            before = s.covered_fraction;
            Proposal { position: s.position, gain_fraction, covered_fraction: s.covered_fraction }
        })
        .collect()
}

type JobResult = Result<Vec<Suggestion>, String>;

/// A running or finished suggestion computation.
#[derive(Debug, Default)]
pub struct SuggestJob {
    running: Option<Job<JobResult>>,
}

impl SuggestJob {
    pub fn start(&mut self, input: SuggestInput) -> Result<(), String> {
        let job = Job::spawn("planning-suggest", move |ctl| compute_in_job(&input, ctl)).map_err(|e| format!("could not start the computation: {e}"))?;
        self.running = Some(job);
        Ok(())
    }

    pub fn progress(&self) -> Option<f32> {
        self.running.as_ref().map(Job::progress)
    }

    /// Stops the computation (dropping the job waits for the worker, at most one grid row).
    pub fn cancel(&mut self) {
        self.running = None;
    }

    /// `Some` once when the computation finishes.
    pub fn poll(&mut self) -> Option<JobResult> {
        let job = self.running.as_mut()?;
        let result = match job.poll() {
            JobPoll::Pending => return None,
            JobPoll::Done(r) => r,
            JobPoll::Cancelled => {
                self.running = None;
                return None;
            }
            JobPoll::Failed => Err("the computation stopped unexpectedly".to_owned()),
        };
        self.running = None;
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::planning::ap_models::builtin_models;
    use crate::planning::predict_input::tests::two_room_project;
    use openspot_heatmap::{CancelToken, Progress};
    use openspot_model::{Wall, WallKind};
    use openspot_predict::{CancelToken as EngineToken, Control};

    fn catalog() -> MaterialCatalog {
        MaterialCatalog::builtin().unwrap()
    }

    fn model() -> ApModel {
        builtin_models().remove(0)
    }

    /// The two-room plan with its only AP removed, so every placement is a first placement.
    fn empty_rooms() -> Project {
        let mut p = two_room_project("concrete-wall");
        p.zones[0].planning.access_points.clear();
        p
    }

    fn input(p: &Project, params: &SuggestParams) -> SuggestInput {
        build_input(p, 0, &catalog(), PropagationModel::default(), 1.0, &model(), params).unwrap()
    }

    fn wait(job: &mut SuggestJob) -> JobResult {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if let Some(r) = job.poll() {
                return r;
            }
            assert!(Instant::now() < deadline, "suggestions never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn the_lattice_never_exceeds_the_engine_limit_and_keeps_requested_spacing_when_it_can() {
        assert_eq!(effective_spacing_m(20.0, 10.0, 2.0), 2.0);
        // 200 m x 200 m at 2 m spacing is 10 000 candidates: coarsened to fit 2 000.
        let s = effective_spacing_m(200.0, 200.0, 2.0);
        assert!(s > 2.0);
        let lattice = candidate_lattice(200.0, 200.0, s, CANDIDATE_MARGIN_M);
        assert!(lattice.len() <= MAX_CANDIDATES, "{} candidates", lattice.len());
        assert!(effective_spacing_m(10.0, 10.0, 0.0) > 0.0, "a zero request never yields a zero spacing");
        // Whatever the size, the engine's candidate limit holds.
        for (w, h) in [(1000.0, 1000.0), (37.3, 912.0), (3.0, 3.0), (200.0, 200.0)] {
            let spacing = effective_spacing_m(w, h, 0.5);
            assert!(candidate_lattice(w, h, spacing, CANDIDATE_MARGIN_M).len() <= MAX_CANDIDATES, "{w} x {h}");
        }
    }

    #[test]
    fn candidates_keep_clear_of_walls() {
        let mut data = PlanningData::default();
        let mut w = Wall::new(Point::new(10.0, 0.0), Point::new(10.0, 10.0), WallKind::Wall, "drywall");
        w.thickness_m = 0.4;
        data.walls.push(w);
        let kept = clear_of_walls(&data, vec![Point::new(9.0, 5.0), Point::new(9.6, 5.0), Point::new(10.4, 5.0), Point::new(10.6, 5.0), Point::new(11.0, 5.0)]);
        // Clearance is 0.3 m plus half the 0.4 m thickness = 0.5 m from the wall's centerline.
        assert_eq!(kept, vec![Point::new(9.0, 5.0), Point::new(10.6, 5.0), Point::new(11.0, 5.0)]);
    }

    #[test]
    fn inputs_take_the_model_radio_for_the_band_and_the_plan_size_from_the_scale() {
        let p = empty_rooms();
        let i = input(&p, &SuggestParams { count: 99, ..SuggestParams::default() });
        assert_eq!((i.template.band, i.template.channel, i.template.width_mhz), (Band::Ghz5, 42, 80));
        assert_eq!((i.count, i.height_m), (MAX_COUNT, model().height_m), "count is clamped");
        assert_eq!((i.grid.width_m, i.grid.height_m), (20.0, 10.0));
        assert!((i.grid.cell_m - 20.0 / 60.0).abs() < 1e-12);
        assert!(i.candidates.iter().all(|c| (CANDIDATE_MARGIN_M..=20.0 - CANDIDATE_MARGIN_M).contains(&c.x)));
        assert!(i.candidates.iter().all(|c| (c.x - 10.0).abs() > 0.3), "none on the dividing wall");
        let wifi6 = build_input(&p, 0, &catalog(), PropagationModel::default(), 1.0, &model(), &SuggestParams { band: Band::Ghz6, ..SuggestParams::default() }).unwrap();
        assert_eq!(wifi6.template.band, Band::Ghz6, "a model without that band still yields a usable radio");
    }

    #[test]
    fn setup_errors_are_reported() {
        let mut p = empty_rooms();
        p.zones[0].scale = None;
        assert_eq!(build_input(&p, 0, &catalog(), PropagationModel::default(), 1.0, &model(), &SuggestParams::default()).unwrap_err(), InputError::NoScale);
        p.zones[0].map = None;
        assert_eq!(build_input(&p, 0, &catalog(), PropagationModel::default(), 1.0, &model(), &SuggestParams::default()).unwrap_err(), InputError::NoPlan);
    }

    #[test]
    fn with_an_ap_in_the_left_room_the_first_suggestion_goes_to_the_right_room() {
        let p = two_room_project("concrete-wall");
        let params = SuggestParams { count: 1, threshold_dbm: -67.0, ..SuggestParams::default() };
        let picks = compute(&input(&p, &params), &Control::default()).unwrap();
        assert_eq!(picks.len(), 1);
        assert!(picks[0].position.x > 10.0, "the uncovered room is on the right: {:?}", picks[0]);
        assert!(picks[0].newly_covered_cells > 0 && picks[0].covered_fraction > 0.5);
    }

    #[test]
    fn gains_are_the_steps_of_the_cumulative_coverage() {
        let p = empty_rooms();
        let picks = compute(&input(&p, &SuggestParams { count: 2, ..SuggestParams::default() }), &Control::default()).unwrap();
        let shown = proposals(&picks);
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].gain_fraction, picks[0].covered_fraction);
        assert!((shown[1].gain_fraction - (picks[1].covered_fraction - picks[0].covered_fraction)).abs() < 1e-12);
        assert!((shown.iter().map(|x| x.gain_fraction).sum::<f64>() - picks[1].covered_fraction).abs() < 1e-12, "gains add up to the total");
        // Against the engine's own cell counts: the first gain is newly covered cells over all cells.
        let cells = picks[0].newly_covered_cells as f64 / picks[0].covered_fraction;
        assert!((shown[1].gain_fraction - picks[1].newly_covered_cells as f64 / cells).abs() < 1e-12);
        assert_eq!(proposals(&[]), vec![]);
    }

    #[test]
    fn on_an_empty_plan_the_first_suggestions_cover_both_rooms() {
        let p = empty_rooms();
        let picks = compute(&input(&p, &SuggestParams { count: 2, ..SuggestParams::default() }), &Control::default()).unwrap();
        assert_eq!(picks.len(), 2);
        assert!((picks[0].position.x < 10.0) != (picks[1].position.x < 10.0), "one per room: {picks:?}");
        assert!(picks[1].covered_fraction > picks[0].covered_fraction);
    }

    #[test]
    fn the_background_result_equals_the_synchronous_one() {
        let p = empty_rooms();
        let i = input(&p, &SuggestParams { count: 2, ..SuggestParams::default() });
        let mut job = SuggestJob::default();
        job.start(i.clone()).unwrap();
        assert!(job.progress().is_some());
        let background = wait(&mut job);
        assert_eq!(background, Ok(compute(&i, &Control::default()).unwrap()));
        assert!(job.progress().is_none() && job.poll().is_none());
    }

    #[test]
    fn cancelling_stops_the_work_and_reports_nothing() {
        let p = empty_rooms();
        let i = input(&p, &SuggestParams::default());
        let (token, progress) = (CancelToken::default(), Progress::default());
        token.cancel();
        assert_eq!(compute_in_job(&i, &JobControl::new(&token, &progress)), Err(Cancelled));
        // The engine itself honours a cancelled token mid-run too.
        let engine = EngineToken::new();
        engine.cancel();
        assert_eq!(compute(&i, &Control { cancel: Some(&engine), progress: None }), Err(openspot_predict::PredictError::Cancelled));
        let mut job = SuggestJob::default();
        job.start(i).unwrap();
        job.cancel();
        assert!(job.progress().is_none() && job.poll().is_none());
    }

    #[test]
    fn engine_problems_come_back_as_text() {
        let mut p = empty_rooms();
        p.zones[0].planning.walls[0].material = "gone".into();
        let i = input(&p, &SuggestParams::default());
        let (token, progress) = (CancelToken::default(), Progress::default());
        let out = compute_in_job(&i, &JobControl::new(&token, &progress)).unwrap();
        assert!(out.unwrap_err().contains("gone"));
    }
}
