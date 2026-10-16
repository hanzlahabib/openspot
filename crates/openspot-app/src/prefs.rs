//! User preferences persisted as JSON in the platform config directory.
//!
//! Loading never fails the app: a missing, unreadable or corrupt file yields defaults (with
//! a warning for the corrupt case). Saving writes a temporary file and renames it, so a
//! crash mid-write cannot destroy the previous preferences.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::active_scan::ActiveScanPrefs;
use crate::aliases::AliasScope;
use crate::groups::CustomGroup;
use crate::theme::Appearance;

/// Bumped when a preference changes meaning; unknown fields from newer versions are ignored.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub version: u32,
    pub inspector: InspectorPrefs,
    pub survey: SurveyPrefs,
    pub appearance: Appearance,
    pub updates: UpdatePrefs,
    /// BSSID ("AA:BB:CC:DD:EE:FF") to user-chosen name, shared by Inspector and Survey.
    pub aliases: BTreeMap<String, String>,
    /// Whether the UI uses `aliases` above or the open project's own list.
    pub alias_scope: AliasScope,
    /// Company, client and logo printed on report covers.
    pub report: openspot_report::ReportBranding,
    /// BCP 47 tag of the chosen interface language; `None` follows the operating system.
    pub language: Option<String>,
    pub planning: PlanningPrefs,
}

/// Planning-mode settings. Every field has a default so older preference files still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanningPrefs {
    /// Access-point models the user saved, next to the built-in generic ones.
    pub ap_models: Vec<crate::planning::ApModel>,
    pub snap_grid: bool,
    /// Index into the planning grid presets.
    pub snap_grid_index: usize,
    pub snap_angle: bool,
    pub snap_endpoints: bool,
    /// Thickness of newly drawn walls, meters.
    pub thickness_m: f64,
    /// Index into `GradientPreset::ALL`.
    pub gradient: usize,
    /// Index into `Smoothing::ALL`. Off by default: predictions are exact per cell and walls
    /// should stay sharp.
    pub smoothing: usize,
    pub opacity: u8,
    /// Recompute the prediction whenever the plan changes.
    pub auto_predict: bool,
    /// Index into the prediction detail levels.
    pub detail: usize,
    /// Include single-bounce wall reflections (slower).
    pub single_bounce: bool,
}

impl Default for PlanningPrefs {
    fn default() -> Self {
        Self { ap_models: Vec::new(), snap_grid: true, snap_grid_index: 1, snap_angle: true, snap_endpoints: true, thickness_m: 0.1, gradient: 0, smoothing: 0, opacity: 170, auto_predict: true, detail: 1, single_bounce: false }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatePrefs {
    /// Check GitHub for a newer release at startup. Off unless the user turns it on.
    pub check_on_startup: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InspectorPrefs {
    pub interval_secs: u64,
    pub window_index: usize,
    pub advisor_width_mhz: u16,
    pub ghz2_4: bool,
    pub ghz5: bool,
    pub ghz6: bool,
    /// `GroupBy::key()` of the table grouping.
    pub group_by: String,
    /// `Column::key()`s shown in the table; `None` means the default set.
    pub visible_columns: Option<Vec<String>>,
    pub custom_groups: Vec<CustomGroup>,
    /// Remove networks not heard for this many seconds; 0 keeps them forever.
    pub stale_after_secs: u64,
    /// Erase event-log entries older than this many seconds; 0 keeps them until the cap.
    pub log_erase_after_secs: u64,
    /// Play a sound when a network appears that was not in the previous scan. Off by default.
    pub sound_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SurveyPrefs {
    /// Index into `GradientPreset::ALL`.
    pub gradient: usize,
    /// Index into `Smoothing::ALL`.
    pub smoothing: usize,
    pub opacity: u8,
    /// Index into `LengthUnit::ALL`.
    pub unit: usize,
    pub show_heatmap: bool,
    pub show_points: bool,
    pub show_ap_markers: bool,
    pub active_scan: ActiveScanPrefs,
    /// Recently opened or saved project files, newest first.
    pub recent: Vec<crate::survey::RecentEntry>,
    /// Seconds between project autosaves; 0 turns autosave off.
    pub autosave_secs: u64,
    /// User-chosen gradient range `(min, max)` per metric key (see `openspot_heatmap::metric_key`).
    pub value_ranges: BTreeMap<String, (f32, f32)>,
    /// Index into `ApMarkerSize::ALL`.
    pub ap_marker_size: usize,
    pub show_ap_titles: bool,
    pub show_path: bool,
    pub show_floor_plan: bool,
    /// Index into `SampleDetail::ALL`.
    pub sample_detail: usize,
    /// Play a sound when a survey measurement succeeds or fails. Off by default.
    pub sound_enabled: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            inspector: InspectorPrefs::default(),
            survey: SurveyPrefs::default(),
            appearance: Appearance::default(),
            updates: UpdatePrefs::default(),
            aliases: BTreeMap::new(),
            alias_scope: AliasScope::default(),
            report: openspot_report::ReportBranding::default(),
            language: None,
            planning: PlanningPrefs::default(),
        }
    }
}

impl Default for InspectorPrefs {
    fn default() -> Self {
        Self {
            interval_secs: 2,
            window_index: 1,
            advisor_width_mhz: 20,
            ghz2_4: true,
            ghz5: true,
            ghz6: true,
            group_by: "none".to_owned(),
            visible_columns: None,
            custom_groups: Vec::new(),
            stale_after_secs: 60,
            log_erase_after_secs: 60 * 60,
            sound_enabled: false,
        }
    }
}

impl Default for SurveyPrefs {
    fn default() -> Self {
        Self {
            gradient: 0,
            smoothing: 1,
            opacity: 170,
            unit: 0,
            show_heatmap: true,
            show_points: true,
            show_ap_markers: true,
            active_scan: ActiveScanPrefs::default(),
            recent: Vec::new(),
            autosave_secs: crate::survey::DEFAULT_AUTOSAVE_SECS,
            value_ranges: BTreeMap::new(),
            ap_marker_size: 1,
            show_ap_titles: true,
            show_path: true,
            show_floor_plan: true,
            sample_detail: 0,
            sound_enabled: false,
        }
    }
}

/// `<config dir>/OpenSpot/prefs.json`, or `None` where the OS has no config directory.
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("OpenSpot").join("prefs.json"))
}

/// Preferences plus a message for the user if something was wrong with the file.
#[derive(Debug)]
pub struct Loaded {
    pub prefs: Prefs,
    pub warning: Option<String>,
}

pub fn load_from(path: &Path) -> Loaded {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded { prefs: Prefs::default(), warning: None },
        Err(e) => {
            return Loaded { prefs: Prefs::default(), warning: Some(format!("could not read preferences: {e}")) };
        }
    };
    match serde_json::from_str::<Prefs>(&text) {
        Ok(prefs) => Loaded { prefs, warning: None },
        Err(e) => Loaded {
            prefs: Prefs::default(),
            warning: Some(format!("preferences file is corrupt ({e}); using defaults")),
        },
    }
}

pub fn save_to(path: &Path, prefs: &Prefs) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(prefs).map_err(io::Error::other)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openspot-prefs-test-{}", openspot_model::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_through_a_file() {
        let dir = temp_dir();
        let path = dir.join("nested").join("prefs.json");
        let mut prefs = Prefs::default();
        prefs.inspector.interval_secs = 10;
        prefs.survey.opacity = 99;
        prefs.aliases.insert("AA:BB:CC:DD:EE:FF".into(), "Office AP".into());
        save_to(&path, &prefs).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.prefs, prefs);
        assert!(loaded.warning.is_none());
        assert!(!path.with_extension("json.tmp").exists(), "temp file must be renamed away");
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn older_survey_prefs_without_visibility_fields_keep_everything_visible() {
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        fs::write(&path, r#"{"survey": {"opacity": 99, "show_points": false}}"#).unwrap();
        let s = load_from(&path).prefs.survey;
        assert_eq!(s.opacity, 99);
        assert!(!s.show_points);
        assert!(s.show_path && s.show_floor_plan && s.show_ap_titles);
        assert_eq!((s.ap_marker_size, s.sample_detail), (1, 0));
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn alias_scope_persists_and_old_files_stay_app_wide() {
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        let prefs = Prefs { alias_scope: AliasScope::Project, ..Prefs::default() };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path).prefs.alias_scope, AliasScope::Project);
        fs::write(&path, r#"{"aliases": {"AA:BB:CC:DD:EE:FF": "Office AP"}}"#).unwrap();
        let old = load_from(&path).prefs;
        assert_eq!(old.alias_scope, AliasScope::App, "files from before the option keep their app-wide list");
        assert_eq!(old.aliases.len(), 1);
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn missing_file_gives_defaults_without_warning() {
        let dir = temp_dir();
        let loaded = load_from(&dir.join("nope.json"));
        assert_eq!(loaded.prefs, Prefs::default());
        assert!(loaded.warning.is_none());
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn corrupt_file_gives_defaults_with_warning_and_is_left_alone() {
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        fs::write(&path, "{ not json").unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.prefs, Prefs::default());
        assert!(loaded.warning.unwrap().contains("corrupt"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn partial_and_future_files_fill_in_defaults() {
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        fs::write(&path, r#"{"inspector": {"interval_secs": 30}, "from_the_future": [1, 2, 3]}"#).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.prefs.inspector.interval_secs, 30);
        assert_eq!(loaded.prefs.inspector.window_index, InspectorPrefs::default().window_index);
        assert_eq!(loaded.prefs.survey, SurveyPrefs::default());
        assert!(loaded.warning.is_none());
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn report_branding_persists_and_older_files_default_it() {
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        let mut prefs = Prefs::default();
        prefs.report.company = "Acme \u{e9}".into();
        prefs.report.logo_path = "C:\\logos\\acme.png".into();
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path).prefs.report, prefs.report);

        fs::write(&path, r#"{"survey": {"opacity": 99}}"#).unwrap();
        let loaded = load_from(&path);
        assert!(loaded.warning.is_none());
        assert!(loaded.prefs.report.is_empty());
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn prefs_written_before_grouping_and_columns_still_load() {
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        // Exactly the inspector fields that existed in schema version 1 before this change.
        fs::write(
            &path,
            r#"{"version":1,"inspector":{"interval_secs":5,"window_index":2,"advisor_width_mhz":40,"ghz2_4":false,"ghz5":true,"ghz6":true}}"#,
        )
        .unwrap();
        let loaded = load_from(&path);
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.prefs.inspector.interval_secs, 5);
        assert!(!loaded.prefs.inspector.ghz2_4);
        assert_eq!(loaded.prefs.inspector.group_by, "none");
        assert_eq!(loaded.prefs.inspector.visible_columns, None);
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn appearance_and_retention_settings_round_trip_and_default_for_old_files() {
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        let prefs = Prefs {
            appearance: crate::theme::Appearance { theme: crate::theme::ThemeChoice::Dark, high_contrast: true, scale_percent: 130 },
            inspector: InspectorPrefs { stale_after_secs: 300, log_erase_after_secs: 0, ..InspectorPrefs::default() },
            ..Prefs::default()
        };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path).prefs, prefs);
        fs::write(&path, r#"{"version":1}"#).unwrap();
        let old = load_from(&path).prefs;
        assert_eq!(old.appearance, crate::theme::Appearance::default());
        assert_eq!((old.inspector.stale_after_secs, old.inspector.log_erase_after_secs), (60, 3600));
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn planning_prefs_round_trip_and_older_files_default_them() {
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        let model = crate::planning::ApModel { name: "Mine".into(), height_m: 3.1, radios: vec![openspot_model::Radio::new(openspot_model::Band::Ghz5, 36, 20, 17.0)] };
        let prefs = Prefs { planning: PlanningPrefs { ap_models: vec![model], snap_grid: false, snap_grid_index: 3, thickness_m: 0.2, gradient: 2, smoothing: 2, opacity: 99, auto_predict: false, detail: 2, single_bounce: true, ..PlanningPrefs::default() }, ..Prefs::default() };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path).prefs, prefs);
        fs::write(&path, r#"{"version":1,"survey":{"opacity":99}}"#).unwrap();
        let old = load_from(&path).prefs;
        assert_eq!(old.planning, PlanningPrefs::default());
        assert!(old.planning.ap_models.is_empty() && old.planning.snap_grid);
        // A file written by a newer version with an extra planning field still loads.
        fs::write(&path, r#"{"planning":{"thickness_m":0.3,"from_the_future":true}}"#).unwrap();
        assert_eq!(load_from(&path).prefs.planning.thickness_m, 0.3);
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn update_check_is_off_unless_saved_on() {
        assert!(!Prefs::default().updates.check_on_startup);
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        fs::write(&path, r#"{"version":1}"#).unwrap();
        assert!(!load_from(&path).prefs.updates.check_on_startup);
        let on = Prefs { updates: UpdatePrefs { check_on_startup: true }, ..Prefs::default() };
        save_to(&path, &on).unwrap();
        assert!(load_from(&path).prefs.updates.check_on_startup);
        let _ = fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn language_is_additive_and_round_trips() {
        let old: Prefs = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert_eq!(old.language, None);
        let dir = temp_dir();
        let path = dir.join("prefs.json");
        save_to(&path, &Prefs { language: Some("de-DE".into()), ..Prefs::default() }).unwrap();
        assert_eq!(load_from(&path).prefs.language.as_deref(), Some("de-DE"));
        fs::remove_dir_all(dir).ok();
    }
}
