//! Turning a project into engine input, and running the engine.
//!
//! Everything here is plain data and pure functions so a prediction can run on a worker
//! thread and so tests can compare a background result with a synchronous one.

use std::sync::atomic::{AtomicUsize, Ordering};

use openspot_heatmap::{Cancelled, Control as JobControl};
use openspot_model::{Band, MaterialCatalog, PlanningData, Project, Uuid};
use openspot_predict::{CancelToken, Control, DEFAULT_RX_HEIGHT_M, GridRequest, Mode, PredictError, PredictionSet, PropagationModel, Scene};

use super::levels::{LevelStack, StackError};

/// Grid resolution choices: label and cells along the plan's longer side. The survey heatmap
/// uses 160; prediction cells are cheaper to be exact about (no interpolation), but cost
/// grows with walls x cells x radios, so the finest option is opt-in.
pub const DETAIL_CELLS: [(&str, f64); 3] = [("Draft", 100.0), ("Normal", 200.0), ("Fine", 400.0)];
pub const DEFAULT_DETAIL: usize = 1;
/// Smallest grid cell: finer than a thin wall's thickness buys nothing.
const MIN_CELL_M: f64 = 0.1;

/// What the user can vary about a prediction besides the plan itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictSettings {
    pub model: PropagationModel,
    pub mode: Mode,
    /// Index into [`DETAIL_CELLS`].
    pub detail: usize,
    pub rx_height_m: f64,
}

impl Default for PredictSettings {
    fn default() -> Self {
        Self { model: PropagationModel::default(), mode: Mode::Direct, detail: DEFAULT_DETAIL, rx_height_m: DEFAULT_RX_HEIGHT_M }
    }
}

/// Why a prediction cannot be started; each message tells the user what to do.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InputError {
    #[error("load or create a floor plan first")]
    NoPlan,
    #[error("set the plan's scale first: predictions need real distances")]
    NoScale,
    #[error(transparent)]
    Stack(#[from] StackError),
}

/// Owned engine input, safe to move to a worker thread.
#[derive(Debug, Clone)]
pub struct PredictInput {
    pub stack: LevelStack,
    pub catalog: MaterialCatalog,
    pub model: PropagationModel,
    pub grid: GridRequest,
}

/// Which radio a grid belongs to, with names for the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioInfo {
    pub ap_id: Uuid,
    pub ap_name: String,
    pub radio_index: usize,
    pub band: Band,
    pub channel: u16,
    pub width_mhz: u16,
    /// Index into the level stack.
    pub level: usize,
}

impl RadioInfo {
    pub fn label(&self) -> String {
        format!("{} · {} ch {}", self.ap_name, self.band.label(), self.channel)
    }
}

/// All radios' predictions over the active level.
#[derive(Debug, Clone, PartialEq)]
pub struct Prediction {
    pub set: PredictionSet,
    /// Parallel to `set.radios`.
    pub radios: Vec<RadioInfo>,
    /// Caveats about the inputs that did not stop the prediction (see [`LevelStack`]).
    pub warnings: Vec<String>,
}

/// Grid cell size in meters for a plan of the given size at `detail`.
pub fn cell_size_m(width_m: f64, height_m: f64, detail: usize) -> f64 {
    let cells = DETAIL_CELLS.get(detail).map_or(DETAIL_CELLS[DEFAULT_DETAIL].1, |d| d.1);
    (width_m.max(height_m) / cells).max(MIN_CELL_M)
}

pub fn build_input(project: &Project, zone_index: usize, catalog: &MaterialCatalog, s: &PredictSettings) -> Result<PredictInput, InputError> {
    let zone = project.zones.get(zone_index).ok_or(StackError::NoZone)?;
    let map = zone.map.as_ref().ok_or(InputError::NoPlan)?;
    let scale = zone.scale.ok_or(InputError::NoScale)?;
    let (width_m, height_m) = (scale.to_meters(f64::from(map.width)), scale.to_meters(f64::from(map.height)));
    let stack = LevelStack::build(project, zone_index)?;
    let grid = GridRequest {
        level: stack.active,
        width_m,
        height_m,
        cell_m: cell_size_m(width_m, height_m, s.detail),
        rx_height_m: s.rx_height_m,
        pixels_per_meter: scale.pixels_per_meter(),
        mode: s.mode,
    };
    Ok(PredictInput { stack, catalog: catalog.clone(), model: s.model, grid })
}

fn radio_infos(stack: &LevelStack, set: &PredictionSet) -> Vec<RadioInfo> {
    set.radios
        .iter()
        .map(|r| {
            let found = stack.levels.iter().enumerate().find_map(|(level, d)| d.access_points.iter().find(|a| a.id == r.ap_id).map(|a| (level, a)));
            let (level, name) = found.map_or((0, String::new()), |(l, a)| (l, a.name.clone()));
            let channel = found.and_then(|(_, a)| a.radios.get(r.radio_index)).map_or(0, |x| x.channel);
            RadioInfo { ap_id: r.ap_id, ap_name: name, radio_index: r.radio_index, band: r.band, channel, width_mhz: r.width_mhz as u16, level }
        })
        .collect()
}

/// Runs the engine. `None` when there is no enabled radio on any level, so there is nothing to show.
pub fn compute(input: &PredictInput, control: &Control<'_>) -> Result<Option<Prediction>, PredictError> {
    let refs: Vec<&PlanningData> = input.stack.levels.iter().collect();
    let scene = Scene::build(&refs, &input.catalog)?;
    let transmitters = Scene::transmitters(&refs)?;
    if transmitters.is_empty() {
        return Ok(None);
    }
    let set = PredictionSet::compute(&scene, &input.model, &transmitters, &input.grid, control)?;
    let radios = radio_infos(&input.stack, &set);
    Ok(Some(Prediction { set, radios, warnings: input.stack.warnings.clone() }))
}

/// Runs engine work under a heatmap job's cancel and progress control, as a worker thread does.
///
/// `callbacks` is how many progress callbacks the engine will make in total (it reports once
/// per finished row of each grid); progress is callbacks seen over that. The engine checks only
/// its own token, so the job's cancellation is forwarded to it on every callback.
/// `Err(Cancelled)` when the job was cancelled; engine errors come back as text.
pub fn in_job<T>(ctl: &JobControl<'_>, callbacks: usize, work: impl FnOnce(&Control<'_>) -> Result<T, PredictError>) -> Result<Result<T, String>, Cancelled> {
    ctl.check()?;
    let token = CancelToken::new();
    let seen = AtomicUsize::new(0);
    let total = callbacks.max(1) as f32;
    let progress = |_: openspot_predict::Progress| {
        if ctl.is_cancelled() {
            token.cancel();
        }
        ctl.report((seen.fetch_add(1, Ordering::Relaxed) + 1) as f32 / total);
    };
    let control = Control { cancel: Some(&token), progress: Some(&progress) };
    match work(&control) {
        Ok(v) => Ok(Ok(v)),
        Err(PredictError::Cancelled) => Err(Cancelled),
        Err(e) => Ok(Err(e.to_string())),
    }
}

/// [`compute`] on a worker thread.
pub fn compute_in_job(input: &PredictInput, ctl: &JobControl<'_>) -> Result<Result<Option<Prediction>, String>, Cancelled> {
    let rows = match input.grid.dimensions() {
        Ok((_, rows)) => rows,
        Err(e) => return Ok(Err(e.to_string())),
    };
    let radios: usize = input.stack.levels.iter().map(|l| l.access_points.iter().map(|a| a.radios.iter().filter(|r| r.enabled).count()).sum::<usize>()).sum();
    in_job(ctl, rows * radios, |control| compute(input, control))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::planning::ap_models::builtin_models;
    use openspot_model::{MapImage, Point, Scale, Wall, WallKind};

    /// A 20 x 10 m plan (2 px per cm: 50 px/m) split into two rooms by a wall at x = 10 m
    /// with one AP in the left room.
    pub(in crate::planning) fn two_room_project(wall_material: &str) -> Project {
        let mut p = Project::new("two rooms");
        let z = &mut p.zones[0];
        z.map = Some(MapImage { original_name: "m".into(), data: vec![], width: 1000, height: 500 });
        z.scale = Scale::new(50.0);
        z.planning.walls.push(Wall::new(Point::new(10.0, 0.0), Point::new(10.0, 10.0), WallKind::Wall, wall_material));
        let mut model = builtin_models().remove(0);
        model.radios.retain(|r| r.band == Band::Ghz5);
        let mut ap = model.instantiate("AP 1", Point::new(5.0, 5.0));
        ap.height_m = 1.0;
        z.planning.access_points.push(ap);
        p
    }

    fn catalog() -> MaterialCatalog {
        MaterialCatalog::builtin().unwrap()
    }

    #[test]
    fn grid_covers_the_plan_in_meters_with_the_zone_scale() {
        let p = two_room_project("drywall");
        let input = build_input(&p, 0, &catalog(), &PredictSettings::default()).unwrap();
        let g = input.grid;
        assert_eq!((g.width_m, g.height_m, g.pixels_per_meter, g.level), (20.0, 10.0, 50.0, 0));
        assert!((g.cell_m - 0.1).abs() < 1e-12, "20 m / 200 cells = 0.1 m");
        let fine = build_input(&p, 0, &catalog(), &PredictSettings { detail: 2, ..PredictSettings::default() }).unwrap();
        assert_eq!(fine.grid.cell_m, 0.1, "never finer than the minimum cell");
        assert_eq!(cell_size_m(100.0, 50.0, 0), 1.0);
        assert_eq!(cell_size_m(100.0, 50.0, 99), 0.5, "an unknown detail index falls back to Normal");
    }

    #[test]
    fn missing_plan_or_scale_is_refused_with_a_message() {
        let mut p = Project::new("p");
        let err = |p: &Project| build_input(p, 0, &catalog(), &PredictSettings::default()).unwrap_err();
        assert_eq!(err(&p), InputError::NoPlan);
        p.zones[0].map = Some(MapImage { original_name: "m".into(), data: vec![], width: 100, height: 100 });
        assert_eq!(err(&p), InputError::NoScale);
        assert!(err(&p).to_string().contains("scale"));
        assert_eq!(build_input(&p, 7, &catalog(), &PredictSettings::default()).unwrap_err(), InputError::Stack(StackError::NoZone));
    }

    #[test]
    fn no_enabled_radio_means_nothing_to_show_not_an_error() {
        let mut p = two_room_project("drywall");
        p.zones[0].planning.access_points[0].radios.iter_mut().for_each(|r| r.enabled = false);
        let input = build_input(&p, 0, &catalog(), &PredictSettings::default()).unwrap();
        assert_eq!(compute(&input, &Control::default()), Ok(None));
        p.zones[0].planning.access_points.clear();
        let input = build_input(&p, 0, &catalog(), &PredictSettings::default()).unwrap();
        assert_eq!(compute(&input, &Control::default()), Ok(None));
    }

    #[test]
    fn unknown_materials_are_reported_by_name() {
        let p = two_room_project("no-such-material");
        let input = build_input(&p, 0, &catalog(), &PredictSettings::default()).unwrap();
        assert_eq!(compute(&input, &Control::default()), Err(PredictError::UnknownMaterial("no-such-material".into())));
    }

    #[test]
    fn radios_carry_ap_names_and_channels() {
        let p = two_room_project("drywall");
        let input = build_input(&p, 0, &catalog(), &PredictSettings::default()).unwrap();
        let pred = compute(&input, &Control::default()).unwrap().unwrap();
        assert_eq!(pred.radios.len(), pred.set.radios.len());
        assert_eq!(pred.radios.len(), 1);
        let r = &pred.radios[0];
        assert_eq!((r.ap_name.as_str(), r.band, r.channel, r.width_mhz, r.level), ("AP 1", Band::Ghz5, 42, 80, 0));
        assert_eq!(r.label(), "AP 1 · 5 GHz ch 42");
    }

    #[test]
    fn job_control_cancellation_stops_the_engine_and_progress_reaches_one() {
        use openspot_heatmap::{CancelToken as JobToken, Progress as JobProgress};
        let p = two_room_project("drywall");
        let input = build_input(&p, 0, &catalog(), &PredictSettings::default()).unwrap();

        let (token, progress) = (JobToken::default(), JobProgress::default());
        let ctl = JobControl::new(&token, &progress);
        let done = compute_in_job(&input, &ctl).unwrap().unwrap().unwrap();
        assert_eq!(progress.get(), 1.0, "every row was reported");
        assert_eq!(done, compute(&input, &Control::default()).unwrap().unwrap(), "same numbers as the plain call");

        token.cancel();
        assert_eq!(compute_in_job(&input, &ctl), Err(Cancelled));
    }

    #[test]
    fn bad_grids_are_reported_as_errors_not_panics() {
        let p = two_room_project("drywall");
        let mut input = build_input(&p, 0, &catalog(), &PredictSettings::default()).unwrap();
        input.grid.cell_m = f64::NAN;
        let (token, progress) = (openspot_heatmap::CancelToken::default(), openspot_heatmap::Progress::default());
        let out = compute_in_job(&input, &JobControl::new(&token, &progress)).unwrap();
        assert!(out.unwrap_err().contains("positive"));
    }
}
