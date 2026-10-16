//! Survey mode: floor plan, calibration, click-to-scan measurements and heatmaps.

mod ap_manual;
mod autosave;
mod calibration;
mod contours;
mod crop;
mod cue;
mod draw;
mod edits;
mod explorer;
mod export;
mod heat_job;
mod hidden;
mod history;
mod merge;
pub(crate) mod network_groups;
mod path;
mod pdf;
mod per_ap;
mod projects;
mod range;
mod sampling;
mod scan;
mod ui;
mod view;
mod visibility;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::TextureHandle;
use openspot_heatmap::{GradientPreset, Metric, Smoothing, SpeedKind};
use openspot_heatmap::rasterize;
use openspot_model::{LengthUnit, LinkInfo, MapImage, Project, ScanPoint, Scale, Snapshot, SpeedResult, Uuid, Zone};
use openspot_store::Store;

use crate::active_scan::ActiveScanState;
use crate::prefs::SurveyPrefs;
use history::{Edit, History};
use openspot_wlan::{AccessPoint, MacAddress};

pub use autosave::{DEFAULT_INTERVAL_SECS as DEFAULT_AUTOSAVE_SECS, MAX_INTERVAL_SECS as MAX_AUTOSAVE_SECS};
pub use calibration::Calibration;
pub use contours::MAX_CONTOUR_LEVELS;
pub use network_groups::GroupFilter;
pub use projects::RecentEntry;
pub use ui::{ReportUi, show};
pub(crate) use ui::save_project;
pub use view::View;
pub use visibility::{ApMarkerSize, SampleDetail};

/// Columns of the heatmap raster along the map's longer side. Fine enough to look
/// smooth once the GPU filters it, small enough to recompute instantly.
pub(crate) const HEATMAP_LONG_SIDE_CELLS: f64 = 160.0;
/// Default AP-count threshold: weaker networks would not be usable by a client.
const AP_COUNT_MIN_RSSI_DBM: i32 = -80;
/// Lowest heatmap opacity offered; below this the overlay is invisible on most plans.
pub(crate) const MIN_OPACITY: u8 = 40;
/// Pixels on the long side of a generated blank plan: sharp when zoomed, cheap to store.
const BLANK_LONG_SIDE_PX: f64 = 1600.0;
/// Largest blank plan edge in meters; beyond this the 1 m grid is below one pixel.
const BLANK_MAX_METERS: f64 = 1000.0;

/// A pending zone/snapshot management action awaiting the user's confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManageAction {
    RenameZone(String),
    RenameSnapshot(String),
    DeleteZone,
    DeleteSnapshot,
}

/// Zoom presets offered in the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomRequest {
    Fit,
    Percent(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pan,
    Measure,
    /// Click waypoints; the app samples along the route automatically.
    Path,
    /// Click the corners of the area the heatmap is limited to.
    Crop,
    /// Draw a plan with lines, rectangles and text.
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricChoice {
    BestSignal,
    SelectedSignal,
    SecondarySignal,
    Sir,
    BandCoverage,
    PhyCoverage,
    ApCount,
    /// Active-scan throughput or jitter map.
    Speed(SpeedKind),
    /// Negotiated transmit rate of the laptop's own connection (H-10).
    TxRate,
}

pub struct Survey {
    pub project: Project,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub zone: usize,
    pub snapshot: usize,
    pub tool: Tool,
    pub calibration: Calibration,
    pub view: View,
    pub metric_choice: MetricChoice,
    pub selected: BTreeSet<MacAddress>,
    /// Which custom group the network list is limited to (session-only, not saved).
    pub group_filter: GroupFilter,
    /// Show only this network's signal map (H-09); overrides the chosen metric.
    pub per_ap: Option<MacAddress>,
    /// Draw vector isolines over the heatmap (H-16).
    pub show_contours: bool,
    pub contour_levels: usize,
    contours: contours::ContourCache,
    /// User gradient ranges per metric key (H-15).
    pub value_ranges: std::collections::BTreeMap<String, (f32, f32)>,
    pub troubleshoot: bool,
    /// Thresholds for the troubleshooting view (same unit as the active metric).
    pub threshold_acceptable: i32,
    pub threshold_critical: i32,
    pub gradient: GradientPreset,
    pub smoothing: Smoothing,
    pub unit: LengthUnit,
    /// Zoom change requested by the toolbar, applied by the canvas (which knows its size).
    pub zoom_request: Option<ZoomRequest>,
    pub show_heatmap: bool,
    pub show_points: bool,
    pub show_ap_markers: bool,
    pub ap_marker_size: ApMarkerSize,
    pub show_ap_titles: bool,
    pub show_path: bool,
    pub show_floor_plan: bool,
    pub sample_detail: SampleDetail,
    pub opacity: u8,
    /// A survey scan is running; further clicks are ignored until it reports back.
    pub busy: bool,
    /// Play a sound when a measurement finishes (opt-in).
    pub sound_enabled: bool,
    /// Cue produced by the last finished measurement, waiting for the app to play it.
    pending_cue: Option<crate::sound::Cue>,
    /// The adapter's connection reported with the latest scan, recorded with the next point.
    scan_link: Option<LinkInfo>,
    pub active: ActiveScanState,
    /// Scan the UI layer must still send to the worker.
    pub pending_scan: Option<(f64, f64)>,
    /// Scans collected so far for the point being measured (several per point if configured).
    sampler: Option<sampling::Sampler>,
    /// AP marker currently being dragged on the canvas.
    pub ap_drag: Option<ap_manual::ApDrag>,
    pub crop_draft: crop::CropDraft,
    pub draw: draw::DrawSession,
    /// Open "draw a plan" dialog with (width m, height m).
    pub draw_dialog: Option<(f64, f64)>,
    /// Open page chooser for a multi-page PDF floor plan.
    pub pdf_choice: Option<pdf::PdfChoice>,
    pub path_run: Option<path::ActiveRun>,
    /// Assumed pace of the person walking a path run, in m/s.
    pub walk_speed_mps: f64,
    pub message: Option<String>,
    pub error: Option<String>,
    /// Report wizard, running export and saved branding.
    pub report: ReportUi,
    /// Open "blank plan" dialog with (width m, height m).
    pub blank_dialog: Option<(f64, f64)>,
    pub alias_edit: Option<(MacAddress, String)>,
    pub manage: Option<ManageAction>,
    /// Open "merge snapshots" dialog with the chosen snapshot indices.
    pub merge_dialog: Option<BTreeSet<usize>>,
    /// Bumped whenever measurement data changes, to invalidate the heatmap cache.
    pub explorer: explorer::ExplorerState,
    heat_jobs: heat_job::HeatJobs,
    pub autosave: autosave::AutosaveState,
    history: History,
    generation: u64,
    /// Opacity baked into the cached heatmap texture; a change forces a rebuild.
    heat_opacity: u8,
    textures: Textures,
    needs_fit: bool,
}

#[derive(Default)]
struct Textures {
    maps: HashMap<Uuid, TextureHandle>,
    heat: Option<(HeatKey, TextureHandle)>,
    /// Grid behind the cached heat texture, for legend statistics.
    heat_grid: Option<openspot_heatmap::Grid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeatKey {
    generation: u64,
    zone: usize,
    snapshot: usize,
    metric: Metric,
    gradient: GradientPreset,
    smoothing: Smoothing,
}

impl Default for Survey {
    fn default() -> Self {
        Self {
            project: Project::new("Untitled survey"),
            path: None,
            dirty: false,
            zone: 0,
            snapshot: 0,
            tool: Tool::Measure,
            calibration: Calibration::Idle,
            view: View::default(),
            metric_choice: MetricChoice::BestSignal,
            selected: BTreeSet::new(),
            group_filter: GroupFilter::default(),
            per_ap: None,
            show_contours: false,
            contour_levels: contours::DEFAULT_CONTOUR_LEVELS,
            contours: contours::ContourCache::default(),
            value_ranges: std::collections::BTreeMap::new(),
            troubleshoot: false,
            threshold_acceptable: -67,
            threshold_critical: -75,
            gradient: GradientPreset::Default,
            smoothing: Smoothing::default(),
            unit: LengthUnit::default(),
            zoom_request: None,
            show_heatmap: true,
            show_points: true,
            show_ap_markers: true,
            ap_marker_size: ApMarkerSize::default(),
            show_ap_titles: true,
            show_path: true,
            show_floor_plan: true,
            sample_detail: SampleDetail::default(),
            opacity: 170,
            busy: false,
            sound_enabled: false,
            pending_cue: None,
            scan_link: None,
            active: ActiveScanState::default(),
            pending_scan: None,
            sampler: None,
            ap_drag: None,
            crop_draft: crop::CropDraft::default(),
            draw: draw::DrawSession::default(),
            draw_dialog: None,
            pdf_choice: None,
            path_run: None,
            walk_speed_mps: path::DEFAULT_WALK_SPEED_MPS,
            message: None,
            error: None,
            report: ReportUi::default(),
            blank_dialog: None,
            alias_edit: None,
            manage: None,
            explorer: explorer::ExplorerState::default(),
            heat_jobs: heat_job::HeatJobs::default(),
            autosave: autosave::AutosaveState::default(),
            merge_dialog: None,
            history: History::default(),
            generation: 0,
            heat_opacity: 0,
            textures: Textures::default(),
            needs_fit: true,
        }
    }
}

/// Whether pixel coordinate `v` falls on a grid line spaced `step` pixels apart.
fn step_hit(v: f64, step: f64) -> bool {
    (v % step) < 1.0
}

fn now_unix() -> i64 {
    // A clock before 1970 is a broken clock; 0 keeps the point usable.
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Survey {
    pub fn prefs(&self) -> SurveyPrefs {
        SurveyPrefs {
            gradient: GradientPreset::ALL.iter().position(|g| *g == self.gradient).unwrap_or(0),
            smoothing: Smoothing::ALL.iter().position(|g| *g == self.smoothing).unwrap_or(1),
            opacity: self.opacity,
            unit: LengthUnit::ALL.iter().position(|u| *u == self.unit).unwrap_or(0),
            show_heatmap: self.show_heatmap,
            show_points: self.show_points,
            show_ap_markers: self.show_ap_markers,
            active_scan: self.active.prefs.clone(),
            recent: self.explorer.recent.clone(),
            autosave_secs: self.autosave.interval_secs,
            value_ranges: self.value_ranges.clone(),
            ap_marker_size: ApMarkerSize::ALL.iter().position(|m| *m == self.ap_marker_size).unwrap_or(1),
            show_ap_titles: self.show_ap_titles,
            show_path: self.show_path,
            show_floor_plan: self.show_floor_plan,
            sample_detail: SampleDetail::ALL.iter().position(|d| *d == self.sample_detail).unwrap_or(0),
            sound_enabled: self.sound_enabled,
        }
    }

    pub fn apply_prefs(&mut self, p: &SurveyPrefs) {
        self.gradient = GradientPreset::ALL.get(p.gradient).copied().unwrap_or_default();
        self.smoothing = Smoothing::ALL.get(p.smoothing).copied().unwrap_or_default();
        self.opacity = p.opacity.clamp(MIN_OPACITY, u8::MAX);
        self.unit = LengthUnit::ALL.get(p.unit).copied().unwrap_or_default();
        self.show_heatmap = p.show_heatmap;
        self.show_points = p.show_points;
        self.show_ap_markers = p.show_ap_markers;
        self.active.apply_prefs(&p.active_scan);
        self.explorer.recent = p.recent.clone();
        self.value_ranges = p.value_ranges.iter().filter(|(_, r)| range::is_valid(**r)).map(|(k, r)| (k.clone(), *r)).collect();
        self.autosave.interval_secs = p.autosave_secs.min(autosave::MAX_INTERVAL_SECS);
        projects::prune_missing(&mut self.explorer.recent, Path::exists);
        self.ap_marker_size = ApMarkerSize::ALL.get(p.ap_marker_size).copied().unwrap_or_default();
        self.show_ap_titles = p.show_ap_titles;
        self.show_path = p.show_path;
        self.show_floor_plan = p.show_floor_plan;
        self.sample_detail = SampleDetail::ALL.get(p.sample_detail).copied().unwrap_or_default();
        self.sound_enabled = p.sound_enabled;
    }

    pub fn zone(&self) -> &Zone {
        &self.project.zones[self.zone]
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.zone().snapshots[self.snapshot]
    }

    fn zone_mut(&mut self) -> &mut Zone {
        &mut self.project.zones[self.zone]
    }

    /// Flags unsaved changes that do not affect any drawn data (e.g. project aliases).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.generation += 1;
    }

    fn reset_session(&mut self) {
        self.zone = 0;
        self.snapshot = 0;
        self.selected.clear();
        self.per_ap = None;
        self.calibration.cancel();
        self.textures = Textures::default();
        self.invalidate_heat();
        self.needs_fit = true;
        self.busy = false;
        self.pending_scan = None;
        self.sampler = None;
        self.ap_drag = None;
        self.crop_draft.reset();
        self.draw = Default::default();
        self.path_run = None;
        self.generation += 1;
        self.history.clear();
    }

    /// Starts an empty project; display preferences and the recent list carry over.
    pub fn new_project(&mut self) {
        // Branding is a user setting, not project data.
        let report = std::mem::take(&mut self.report);
        let (prefs, explorer_visible) = (self.prefs(), self.explorer.visible);
        *self = Self::default();
        self.report.branding = report.branding;
        self.apply_prefs(&prefs);
        self.explorer.visible = explorer_visible;
        self.explorer.start_page = false;
    }

    pub fn select_zone(&mut self, index: usize) {
        if index < self.project.zones.len() && index != self.zone {
            self.zone = index;
            self.crop_draft.reset();
            self.snapshot = 0;
            self.selected.clear();
            self.per_ap = None;
            self.needs_fit = true;
            self.generation += 1;
        }
    }

    pub fn select_snapshot(&mut self, index: usize) {
        if index < self.zone().snapshots.len() && index != self.snapshot {
            self.snapshot = index;
            self.selected.clear();
            self.per_ap = None;
            self.generation += 1;
        }
    }

    /// Deletes the current zone with everything in it. A project always keeps one zone.
    pub fn delete_zone(&mut self) -> bool {
        if self.project.zones.len() <= 1 {
            return false;
        }
        let removed = self.project.zones.remove(self.zone);
        self.project.structure.forget_zone(removed.id);
        self.textures.maps.remove(&removed.id);
        self.zone = self.zone.min(self.project.zones.len() - 1);
        self.snapshot = 0;
        self.selected.clear();
        self.needs_fit = true;
        // Edits refer to zones by index, which just shifted.
        self.history.clear();
        self.touch();
        true
    }

    pub fn add_zone(&mut self) {
        let name = format!("Zone {}", self.project.zones.len() + 1);
        self.project.zones.push(Zone::new(name));
        self.select_zone(self.project.zones.len() - 1);
        self.touch();
    }

    /// Passive-only shorthand for [`Survey::add_scan_with_speed`], used by tests.
    #[cfg(test)]
    pub fn add_scan(&mut self, x: f64, y: f64, access_points: Vec<AccessPoint>) {
        self.add_scan_with_speed(x, y, access_points, Vec::new());
    }

    /// Records a finished scan at map position `(x, y)` with the point's active-scan results,
    /// as one edit so undo and redo keep them together.
    pub fn add_scan_with_speed(&mut self, x: f64, y: f64, access_points: Vec<AccessPoint>, speed: Vec<SpeedResult>) {
        let (zone, snapshot) = (self.zone, self.snapshot);
        let mut point = ScanPoint::new(x, y, now_unix(), access_points);
        point.speed = speed;
        point.link = self.scan_link.take();
        self.zone_mut().snapshots[snapshot].points.push(point.clone());
        self.history.record(Edit::PointAdded { zone, snapshot, point });
        self.touch();
    }

    /// Loads an image file as the current zone's floor plan.
    pub fn load_map(&mut self, path: &Path) -> Result<(), String> {
        let data = std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let img = image::load_from_memory(&data).map_err(|e| format!("not a supported image: {e}"))?;
        let original_name = path.file_name().map_or_else(|| "map".into(), |n| n.to_string_lossy().into_owned());
        self.install_map(MapImage { original_name, data, width: img.width(), height: img.height() });
        Ok(())
    }

    /// Makes `map` the current zone's floor plan: the scale is unknown until calibrated and
    /// a drawn plan is dropped.
    pub(crate) fn install_map(&mut self, map: MapImage) {
        let zone = self.zone_mut();
        zone.map = Some(map);
        zone.scale = None;
        let id = zone.id;
        self.discard_drawing();
        self.textures.maps.remove(&id);
        self.needs_fit = true;
        self.history.clear();
        self.touch();
    }

    /// Replaces the current zone's floor plan with an empty grid of the given real size
    /// (1 m minor lines, 5 m major lines) and sets the scale to match, so no calibration
    /// is needed.
    pub fn create_blank_map(&mut self, width_m: f64, height_m: f64) -> Result<(), String> {
        let valid = |v: f64| v.is_finite() && (1.0..=BLANK_MAX_METERS).contains(&v);
        if !valid(width_m) || !valid(height_m) {
            return Err(format!("size must be between 1 and {BLANK_MAX_METERS} meters"));
        }
        let ppm = BLANK_LONG_SIDE_PX / width_m.max(height_m);
        let (w, h) = ((width_m * ppm).round() as u32, (height_m * ppm).round() as u32);
        let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([250, 250, 250, 255]));
        for (x, y, px) in img.enumerate_pixels_mut() {
            let on_line = |v: u32, step: u32| step_hit(f64::from(v), ppm * f64::from(step));
            if on_line(x, 5) || on_line(y, 5) {
                *px = image::Rgba([185, 185, 185, 255]);
            } else if ppm >= 8.0 && (on_line(x, 1) || on_line(y, 1)) {
                *px = image::Rgba([225, 225, 225, 255]);
            }
        }
        let mut data = std::io::Cursor::new(Vec::new());
        img.write_to(&mut data, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        let zone = self.zone_mut();
        zone.map = Some(MapImage { original_name: format!("blank {width_m}x{height_m} m"), data: data.into_inner(), width: w, height: h });
        zone.scale = Scale::new(ppm);
        let id = zone.id;
        self.discard_drawing();
        self.textures.maps.remove(&id);
        self.needs_fit = true;
        self.history.clear();
        self.touch();
        Ok(())
    }

    /// Saves the project. If the file already exists its previous contents are first copied
    /// to `<file>.bak`, so a bad save (or a change of mind) never loses the last good version.
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        if path.exists() {
            let mut backup = path.as_os_str().to_owned();
            backup.push(".bak");
            std::fs::copy(path, &backup).map_err(|e| format!("could not back up {}: {e}", path.display()))?;
        }
        self.project.structure.stamp_saved(now_unix());
        let mut store = Store::open(path).map_err(|e| e.to_string())?;
        store.save(&self.project).map_err(|e| e.to_string())?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.note_recent(path);
        autosave::clear_autosave(path);
        self.reset_autosave_timer();
        Ok(())
    }

    pub fn open_from(&mut self, path: &Path) -> Result<(), String> {
        let project = Store::open(path).and_then(|s| s.load()).map_err(|e| e.to_string())?;
        self.project = project;
        self.project.structure.prune(&self.project.zones);
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.reset_session();
        self.note_recent(path);
        self.explorer.start_page = false;
        self.autosave.recovery = autosave::recovery_candidate(path);
        self.reset_autosave_timer();
        Ok(())
    }

    fn base_metric(&self) -> Metric {
        if let Some(b) = self.per_ap {
            return Metric::Signal(vec![b]);
        }
        match self.metric_choice {
            MetricChoice::BestSignal => Metric::Signal(Vec::new()),
            MetricChoice::SelectedSignal => Metric::Signal(self.selected.iter().copied().collect()),
            MetricChoice::SecondarySignal => Metric::SecondarySignal(self.selected.iter().copied().collect()),
            MetricChoice::Sir => Metric::Sir(self.selected.iter().copied().collect()),
            MetricChoice::BandCoverage => Metric::BandCoverage {
                wanted: self.selected.iter().copied().collect(),
                min_rssi_dbm: AP_COUNT_MIN_RSSI_DBM,
            },
            MetricChoice::PhyCoverage => Metric::PhyCoverage(self.selected.iter().copied().collect()),
            MetricChoice::ApCount => Metric::ApCount { min_rssi_dbm: AP_COUNT_MIN_RSSI_DBM },
            MetricChoice::Speed(kind) => Metric::Speed { kind, ssids: Vec::new() },
            MetricChoice::TxRate => Metric::TxRate,
        }
    }

    /// The metric to draw: the chosen one, wrapped in the troubleshooting view if enabled
    /// and supported.
    pub fn metric(&self) -> Metric {
        let base = self.base_metric();
        if self.troubleshoot && base.default_thresholds().is_some() {
            Metric::Troubleshoot {
                base: Box::new(base),
                acceptable: self.threshold_acceptable,
                critical: self.threshold_critical,
            }
        } else {
            base
        }
    }

    /// Unit of the underlying (non-troubleshooting) metric, e.g. "dBm".
    pub fn base_metric_unit(&self) -> &'static str {
        self.base_metric().unit()
    }

    pub fn supports_troubleshooting(&self) -> bool {
        self.base_metric().default_thresholds().is_some()
    }

    /// Resets the troubleshooting thresholds to the active metric's defaults.
    pub fn reset_thresholds(&mut self) {
        if let Some((acceptable, critical)) = self.base_metric().default_thresholds() {
            self.threshold_acceptable = acceptable;
            self.threshold_critical = critical;
        }
    }

    fn heat_key(&self) -> HeatKey {
        HeatKey { generation: self.generation, zone: self.zone, snapshot: self.snapshot, metric: self.metric(), gradient: self.gradient, smoothing: self.smoothing }
    }

    /// Estimated positions of the selected networks (needs three or more readings each).
    pub fn ap_estimates(&self) -> Vec<(MacAddress, String, openspot_heatmap::ApEstimate)> {
        let snapshot = self.filtered_snapshot();
        let names: std::collections::BTreeMap<_, _> = snapshot.networks().into_iter().map(|(b, s, _)| (b, s)).collect();
        self.selected
            .iter()
            .filter_map(|b| {
                let est = openspot_heatmap::estimate_ap_position(&snapshot, b)?;
                Some((*b, names.get(b).cloned().unwrap_or_default(), est))
            })
            .collect()
    }

    /// Text for the heatmap value under map position `(x, y)`: a number with unit for
    /// continuous metrics, the class label for categorical ones.
    pub fn heat_text_at(&self, x: f64, y: f64) -> Option<String> {
        if !self.show_heatmap || x < 0.0 || y < 0.0 {
            return None;
        }
        let grid = self.textures.heat_grid.as_ref()?;
        let value = crate::heat_ui::value_at(grid, x, y)?;
        Some(crate::heat_ui::value_label(&self.metric(), value))
    }

    /// How far a measurement point's value is trusted to reach, in map pixels.
    pub(crate) fn influence_radius_px(&self) -> f64 {
        let long = self.zone().map.as_ref().map_or(0.0, |m| f64::from(m.width.max(m.height)));
        sampling::extend_px(self.radius_preset(), self.zone().scale, long)
    }

    /// Synchronous grid for the current snapshot and metric, with the crop applied. Exports
    /// use this; the canvas renders in the background instead (see `heat_job`).
    pub(crate) fn compute_grid(&self) -> Option<openspot_heatmap::Grid> {
        let mut grid = rasterize(&self.filtered_snapshot(), &self.metric(), &self.heat_spec()?, self.smoothing)?;
        self.crop_interpolated(&mut grid);
        Some(grid)
    }
}

#[cfg(test)]
mod canvas_tests;
#[cfg(test)]
mod speed_tests;
#[cfg(test)]
mod tests;
