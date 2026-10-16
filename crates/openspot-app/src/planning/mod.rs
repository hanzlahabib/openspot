//! Planning mode: draw walls and place virtual access points on a zone's floor plan and
//! predict coverage with `openspot-predict`.
//!
//! The floor-plan image and scale are the zone's, shared with Survey; the geometry lives in
//! `Zone::planning` in meters. This module holds the session state and the logic that is
//! independent of drawing; `ui/` is the egui layer.

mod ap_edit;
mod ap_models;
mod calibrate_fit;
mod channels;
mod edit;
mod geometry;
mod levels;
mod materials;
mod ops;
mod predict_input;
mod predict_job;
mod results;
mod session;
mod suggest;
mod tools;
mod ui;

use std::collections::HashMap;

use openspot_model::{MaterialCatalog, Point, Project, Scale, Uuid, VirtualAp, Zone};

use openspot_heatmap::{GradientPreset, Smoothing};
use openspot_predict::Mode;

use crate::prefs::PlanningPrefs;
use crate::survey::View;
pub use ap_models::ApModel;
use ap_models::ModelLibrary;
use edit::{Edit, History};
use geometry::{GRID_PRESETS_M, SnapSettings};
use materials::MaterialDraft;
use ops::Selection;
use predict_input::PredictSettings;
use predict_job::PredictJobs;
use results::ViewSettings;
use session::{CalibrationState, HeatCache, Shown, SuggestState};
use tools::{Context, Editor, Outcome, Pointer, Tool, ToolSettings};
pub use ui::show;

/// Pick and snap distance in screen pixels; converted to meters at the current zoom.
pub(crate) const PICK_TOLERANCE_PX: f64 = 8.0;

/// Material editor window state.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialEditor {
    /// Custom material being edited, or `None` while creating a new one.
    pub editing: Option<String>,
    pub draft: MaterialDraft,
    pub error: Option<String>,
}

pub struct Planning {
    pub editor: Editor,
    pub settings: ToolSettings,
    pub models: ModelLibrary,
    pub catalog: MaterialCatalog,
    history: History,
    pub view: View,
    pub needs_fit: bool,
    pub zoom_fit_requested: bool,
    /// A primary-button drag on empty space is panning the view.
    pan_drag: bool,
    /// Text shown in the status bar: the last refusal or result.
    pub notice: Option<String>,
    pub material_editor: Option<MaterialEditor>,
    /// The selected AP being edited in the side panel: a copy of the stored AP as last seen,
    /// and the working copy. Edits commit as one undo step when a field is released.
    pub ap_draft: Option<(VirtualAp, VirtualAp)>,
    /// Name typed for "save as model".
    pub model_name: String,
    /// Plan width in meters typed by the user to set a missing scale.
    pub assumed_width_m: f64,
    /// Blank-plan dialog (width, height in meters).
    pub blank_dialog: Option<(f64, f64)>,
    /// Zone shown last frame; a change resets selection and half-drawn shapes.
    last_zone: Option<Uuid>,
    last_project: Option<Uuid>,
    /// Bumped on every change of planning data, so cached results know when to go stale.
    generation: u64,
    /// Project changed since the app last asked (it marks the project unsaved).
    changed: bool,
    textures: HashMap<Uuid, ui::MapTexture>,
    pub predict_settings: PredictSettings,
    /// Recompute automatically when the plan changes.
    pub auto_predict: bool,
    /// "Predict now" was pressed; consumed by the next frame.
    pub predict_requested: bool,
    /// Draw the prediction over the plan.
    pub show_prediction: bool,
    /// Draw the floor below faintly, to line up stairwells.
    pub show_floor_below: bool,
    pub view_settings: ViewSettings,
    pub calibration: CalibrationState,
    pub suggest: SuggestState,
    jobs: PredictJobs,
    shown: Option<Shown>,
    shown_serial: u64,
    /// Why the last prediction attempt produced nothing.
    pub predict_error: Option<String>,
    heat: Option<HeatCache>,
}

impl std::fmt::Debug for Planning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Planning").field("tool", &self.editor.tool()).field("generation", &self.generation).finish_non_exhaustive()
    }
}

impl Default for Planning {
    fn default() -> Self {
        Self::new(&PlanningPrefs::default())
    }
}

impl Planning {
    pub fn new(prefs: &PlanningPrefs) -> Self {
        // The catalog is compiled in and tested to parse; an empty one would only mean
        // an empty picker, never a crash.
        let (catalog, catalog_error) = match MaterialCatalog::builtin() {
            Ok(c) => (c, None),
            Err(e) => (MaterialCatalog { version: 0, provenance: String::new(), materials: Vec::new() }, Some(format!("the built-in material list could not be loaded: {e}"))),
        };
        let models = ModelLibrary::new(prefs.ap_models.clone());
        let settings = ToolSettings::new(models.builtin()[0].clone());
        let mut planning = Self {
            editor: Editor::default(),
            settings,
            models,
            catalog,
            history: History::default(),
            view: View::default(),
            needs_fit: true,
            zoom_fit_requested: false,
            pan_drag: false,
            notice: catalog_error,
            material_editor: None,
            ap_draft: None,
            model_name: String::new(),
            assumed_width_m: 20.0,
            blank_dialog: None,
            last_zone: None,
            last_project: None,
            generation: 0,
            changed: false,
            textures: HashMap::new(),
            predict_settings: PredictSettings::default(),
            auto_predict: true,
            predict_requested: false,
            show_prediction: true,
            show_floor_below: true,
            view_settings: ViewSettings::default(),
            calibration: CalibrationState::default(),
            suggest: SuggestState::default(),
            jobs: PredictJobs::default(),
            shown: None,
            shown_serial: 0,
            predict_error: None,
            heat: None,
        };
        planning.apply_prefs(prefs);
        planning
    }

    pub fn prefs(&self) -> PlanningPrefs {
        PlanningPrefs {
            ap_models: self.models.user.clone(),
            snap_grid: self.settings.snap.grid,
            snap_grid_index: GRID_PRESETS_M.iter().position(|g| *g == self.settings.snap.grid_m).unwrap_or(1),
            snap_angle: self.settings.snap.angle,
            snap_endpoints: self.settings.snap.endpoints,
            thickness_m: self.settings.thickness_m,
            gradient: GradientPreset::ALL.iter().position(|g| *g == self.view_settings.gradient).unwrap_or(0),
            smoothing: Smoothing::ALL.iter().position(|g| *g == self.view_settings.smoothing).unwrap_or(0),
            opacity: self.view_settings.opacity,
            auto_predict: self.auto_predict,
            detail: self.predict_settings.detail,
            single_bounce: self.predict_settings.mode == Mode::SingleBounce,
        }
    }

    pub fn apply_prefs(&mut self, p: &PlanningPrefs) {
        self.models.user = p.ap_models.clone();
        let grid_m = GRID_PRESETS_M.get(p.snap_grid_index).copied().unwrap_or(GRID_PRESETS_M[1]);
        self.settings.snap = SnapSettings { grid: p.snap_grid, grid_m, angle: p.snap_angle, endpoints: p.snap_endpoints };
        if p.thickness_m.is_finite() && p.thickness_m > 0.0 && p.thickness_m <= tools::MAX_THICKNESS_M {
            self.settings.thickness_m = p.thickness_m;
        }
        self.view_settings.gradient = GradientPreset::ALL.get(p.gradient).copied().unwrap_or_default();
        self.view_settings.smoothing = Smoothing::ALL.get(p.smoothing).copied().unwrap_or(Smoothing::Off);
        self.view_settings.opacity = p.opacity.clamp(crate::survey::MIN_OPACITY, u8::MAX);
        self.auto_predict = p.auto_predict;
        self.predict_settings.detail = p.detail.min(predict_input::DETAIL_CELLS.len() - 1);
        self.predict_settings.mode = if p.single_bounce { Mode::SingleBounce } else { Mode::Direct };
    }

    #[cfg(test)]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether planning data changed since the last call; the app marks the project dirty.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Resets transient state when the shown zone changes, and the history when another
    /// project was opened (its edits would not apply to the new one).
    pub fn sync_zone(&mut self, project: Uuid, zone: Uuid) {
        if self.last_project != Some(project) {
            self.last_project = Some(project);
            self.history.clear();
            self.generation += 1;
        }
        if self.last_zone != Some(zone) {
            self.last_zone = Some(zone);
            self.editor.cancel();
            self.editor.selection = None;
            self.needs_fit = true;
        }
    }

    /// Applies and records an edit. Returns false (and changes nothing) if it no longer
    /// fits the project.
    pub fn commit(&mut self, project: &mut Project, edit: Edit) -> bool {
        if !edit.apply(project, true) {
            return false;
        }
        self.history.record(edit);
        // A refusal shown earlier is about an action that has since been superseded.
        self.notice = None;
        self.after_change(project);
        true
    }

    fn after_change(&mut self, project: &Project) {
        self.generation += 1;
        self.changed = true;
        if let Some(zone) = self.last_zone.and_then(|id| project.zones.iter().find(|z| z.id == id)) {
            self.editor.refresh_selection(&zone.planning);
        }
    }

    fn take_outcome(&mut self, project: &mut Project, out: Outcome) {
        if let Some(message) = out.message {
            self.notice = Some(message);
        }
        for edit in out.edits {
            self.commit(project, edit);
        }
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        let done = self.history.undo(project);
        if done {
            self.after_change(project);
        }
        done
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        let done = self.history.redo(project);
        if done {
            self.after_change(project);
        }
        done
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.notice = None;
        self.editor.set_tool(tool);
    }

    /// Feeds one pointer event, in plan meters, to the active tool and applies the result.
    pub fn pointer(&mut self, project: &mut Project, zone: usize, kind: Pointer, pos: Point, tol_m: f64) {
        let Some(z) = project.zones.get(zone) else { return };
        let ctx = Context { zone: z.id, data: &z.planning, settings: &self.settings, tol_m };
        let out = self.editor.pointer(kind, pos, &ctx);
        self.take_outcome(project, out);
    }

    pub fn confirm(&mut self, project: &mut Project, zone: usize, tol_m: f64) {
        let Some(z) = project.zones.get(zone) else { return };
        let ctx = Context { zone: z.id, data: &z.planning, settings: &self.settings, tol_m };
        let out = self.editor.confirm(&ctx);
        self.take_outcome(project, out);
    }

    pub fn delete_selection(&mut self, project: &mut Project, zone: usize, tol_m: f64) {
        let Some(z) = project.zones.get(zone) else { return };
        let ctx = Context { zone: z.id, data: &z.planning, settings: &self.settings, tol_m };
        let out = self.editor.delete_selection(&ctx);
        self.take_outcome(project, out);
    }

    /// Edits wall `id` of `zone` with `f` (one undo step).
    pub fn edit_wall(&mut self, project: &mut Project, zone: usize, id: Uuid, f: impl FnOnce(&mut openspot_model::Wall)) {
        let Some(z) = project.zones.get(zone) else { return };
        if let Some(edit) = ops::modify_wall(z.id, &z.planning, id, f) {
            self.commit(project, edit);
        }
    }

    // ---- custom materials ------------------------------------------------------------

    pub fn open_material_editor(&mut self) {
        if self.material_editor.is_none() {
            self.material_editor = Some(MaterialEditor { editing: None, draft: MaterialDraft::new_wall(), error: None });
        }
    }

    /// Saves the material editor's draft: a new material, or an update of the edited one.
    pub fn save_material(&mut self, project: &mut Project, zone: usize) -> bool {
        let Some(editor) = self.material_editor.clone() else { return false };
        let Some(z) = project.zones.get(zone) else { return false };
        let result = match &editor.editing {
            Some(id) => materials::update_edit(z.id, &z.planning, id, &editor.draft).map(|e| (e, None)),
            None => materials::add_edit(z.id, &z.planning, &self.catalog, &editor.draft).map(|(e, id)| (e, Some(id))),
        };
        match result {
            Ok((edit, new_id)) => {
                self.commit(project, edit);
                if let Some(id) = new_id {
                    self.material_editor = Some(MaterialEditor { editing: Some(id), ..editor });
                }
                if let Some(e) = &mut self.material_editor {
                    e.error = None;
                }
                true
            }
            Err(e) => {
                if let Some(me) = &mut self.material_editor {
                    me.error = Some(e.to_string());
                }
                false
            }
        }
    }

    pub fn delete_material(&mut self, project: &mut Project, zone: usize, id: &str) {
        let Some(z) = project.zones.get(zone) else { return };
        match materials::remove_edit(z.id, &z.planning, id) {
            Ok(edit) => {
                self.commit(project, edit);
                self.material_editor = None;
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    /// Copies custom material `id` to the other floors of the zone's building.
    pub fn copy_material_to_building(&mut self, project: &mut Project, zone: usize, id: &str) {
        let Some(z) = project.zones.get(zone) else { return };
        let zones: Vec<Uuid> = match project.structure.building_of(z.id).and_then(|b| project.structure.buildings.get(b)) {
            Some(b) => b.floors.iter().map(|f| f.zone).collect(),
            None => {
                self.notice = Some("this zone is not part of a building; group floors in the Survey explorer first".into());
                return;
            }
        };
        match materials::copy_to_zones_edit(project, z.id, id, &zones) {
            Some(edit) => {
                self.commit(project, edit);
                self.notice = Some("copied to the other floors of this building".into());
            }
            None => self.notice = Some("every floor of this building already has it".into()),
        }
    }

    // ---- access points ---------------------------------------------------------------

    /// Replaces access point `id` of `zone` with `ap` (one undo step), after sanitizing its radios.
    pub fn commit_ap(&mut self, project: &mut Project, zone: usize, id: Uuid, mut ap: VirtualAp) {
        ap.radios.iter_mut().for_each(ap_edit::sanitize);
        ap.height_m = ap.height_m.clamp(*ap_edit::HEIGHT_RANGE_M.start(), *ap_edit::HEIGHT_RANGE_M.end());
        let Some(z) = project.zones.get(zone) else { return };
        if let Some(edit) = ops::modify_ap(z.id, &z.planning, id, |a| *a = ap) {
            self.commit(project, edit);
        }
    }

    /// Makes the settings of access point `ap` a saved model called `name`.
    pub fn save_model(&mut self, ap: &VirtualAp, name: &str) {
        match self.models.save_user(ApModel::from_ap(name, ap)) {
            Ok(()) => self.notice = Some(format!("saved model '{}'", name.trim())),
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    /// Sets the material of the slab under the zone's level.
    pub fn set_slab_material(&mut self, project: &mut Project, zone: usize, material: &str) {
        let Some(z) = project.zones.get(zone) else { return };
        if let Some(edit) = ops::slab_material_edit(z.id, &z.planning, material) {
            self.commit(project, edit);
        }
    }

    // ---- scale -----------------------------------------------------------------------

    /// Sets the zone's scale so its floor plan is `width_m` meters wide.
    pub fn set_scale_from_width(zone: &mut Zone, width_m: f64) -> Result<Scale, String> {
        let map = zone.map.as_ref().ok_or("load a floor plan first")?;
        if !(width_m.is_finite() && width_m > 0.0) {
            return Err("width must be a positive number of meters".into());
        }
        let scale = Scale::new(f64::from(map.width) / width_m).ok_or("that width gives an unusable scale")?;
        zone.scale = Some(scale);
        Ok(scale)
    }

    /// Currently selected element, for the property panel.
    pub fn selection(&self) -> Option<Selection> {
        self.editor.selection
    }
}

#[cfg(test)]
mod e2e;
#[cfg(test)]
mod tests;
