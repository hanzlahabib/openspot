//! Timed autosave. The project is written to `<project>.autosave` (never to the main file),
//! removed on every successful manual save and offered for recovery when it is newer than
//! the file it belongs to.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use openspot_model::Project;
use openspot_store::Store;

use super::Survey;

/// Default time between autosaves: short enough to lose little work, long enough that
/// writing a map-heavy project is not noticeable.
pub const DEFAULT_INTERVAL_SECS: u64 = 120;
/// Longest interval the UI offers (an hour); beyond that autosave is not useful.
pub const MAX_INTERVAL_SECS: u64 = 3600;

#[derive(Debug)]
pub struct AutosaveState {
    /// Seconds between autosaves; 0 disables.
    pub interval_secs: u64,
    last: Instant,
    /// An autosave newer than the opened file, waiting for the user's decision.
    pub recovery: Option<PathBuf>,
}

impl Default for AutosaveState {
    fn default() -> Self {
        Self { interval_secs: DEFAULT_INTERVAL_SECS, last: Instant::now(), recovery: None }
    }
}

/// `<project>.autosave` next to the project file.
pub fn autosave_path(project: &Path) -> PathBuf {
    with_suffix(project, ".autosave")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s: OsString = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Whether an autosave is due: something is unsaved, there is a file to attach it to,
/// autosave is on and the interval has elapsed.
pub fn is_due(dirty: bool, has_path: bool, interval_secs: u64, since_last: Duration) -> bool {
    dirty && has_path && interval_secs > 0 && since_last >= Duration::from_secs(interval_secs)
}

/// An autosave is worth offering only if it was written after the main file was last saved
/// (or the main file is gone). `None` times mean the file does not exist.
pub fn autosave_is_newer(autosave: Option<SystemTime>, main: Option<SystemTime>) -> bool {
    match (autosave, main) {
        (Some(a), Some(m)) => a > m,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The autosave of `project` if one exists and is newer than the project file.
pub fn recovery_candidate(project: &Path) -> Option<PathBuf> {
    let auto = autosave_path(project);
    autosave_is_newer(modified(&auto), modified(project)).then_some(auto)
}

/// Writes `project` to `<main>.autosave` through a temporary file and a rename, so an
/// interrupted write leaves the previous autosave intact.
pub fn write_autosave(project: &Project, main: &Path) -> Result<PathBuf, String> {
    let target = autosave_path(main);
    let tmp = with_suffix(&target, ".tmp");
    // A crash can leave a half-written temp file behind; start from scratch.
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("could not clear {}: {e}", tmp.display())),
    }
    {
        let mut store = Store::open(&tmp).map_err(|e| e.to_string())?;
        store.save(project).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &target).map_err(|e| format!("could not write {}: {e}", target.display()))?;
    Ok(target)
}

/// Removes the autosave of `main` if present.
pub fn clear_autosave(main: &Path) {
    // Nothing to report: a missing file is the goal, and a failure only leaves a stale
    // autosave that the newer-than check will not offer.
    let _ = std::fs::remove_file(autosave_path(main));
}

impl Survey {
    /// Called every frame; writes an autosave when one is due. Failures surface as the
    /// survey error line but never touch the main file.
    pub fn tick_autosave(&mut self, now: Instant) {
        let due = is_due(self.dirty, self.path.is_some(), self.autosave.interval_secs, now.saturating_duration_since(self.autosave.last));
        if !due {
            return;
        }
        self.autosave.last = now;
        let Some(path) = self.path.clone() else { return };
        if let Err(e) = write_autosave(&self.project, &path) {
            self.error = Some(format!("autosave failed: {e}"));
        }
    }

    /// Restarts the autosave timer (after open / manual save).
    pub(super) fn reset_autosave_timer(&mut self) {
        self.autosave.last = Instant::now();
    }

    /// Replaces the project with the pending autosave; the result counts as unsaved.
    pub fn recover_autosave(&mut self) -> Result<(), String> {
        let Some(auto) = self.autosave.recovery.take() else { return Ok(()) };
        let project = Store::open(&auto).and_then(|s| s.load()).map_err(|e| format!("could not read autosave: {e}"))?;
        self.project = project;
        self.project.structure.prune(&self.project.zones);
        self.reset_session();
        self.dirty = true;
        Ok(())
    }

    /// Throws away the pending autosave.
    pub fn discard_autosave(&mut self) {
        if let Some(path) = &self.path {
            clear_autosave(path);
        }
        self.autosave.recovery = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openspot-autosave-{}", openspot_model::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn due_only_when_dirty_attached_enabled_and_elapsed() {
        let secs = Duration::from_secs;
        assert!(is_due(true, true, 60, secs(60)));
        assert!(!is_due(true, true, 60, secs(59)), "interval not elapsed");
        assert!(!is_due(false, true, 60, secs(600)), "nothing unsaved");
        assert!(!is_due(true, false, 60, secs(600)), "no file to attach to");
        assert!(!is_due(true, true, 0, secs(600)), "disabled");
    }

    #[test]
    fn newer_check_covers_all_cases() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let t1 = t0 + Duration::from_secs(1);
        assert!(autosave_is_newer(Some(t1), Some(t0)));
        assert!(!autosave_is_newer(Some(t0), Some(t1)));
        assert!(!autosave_is_newer(Some(t0), Some(t0)));
        assert!(autosave_is_newer(Some(t0), None));
        assert!(!autosave_is_newer(None, Some(t0)));
        assert!(!autosave_is_newer(None, None));
    }

    #[test]
    fn autosave_never_touches_the_main_file_and_is_cleared_by_manual_save() {
        let dir = temp_dir();
        let main = dir.join("site.ospot");
        let mut s = Survey::default();
        s.save_to(&main).unwrap();
        let before = std::fs::read(&main).unwrap();
        s.add_scan(5.0, 6.0, vec![]);
        // Interval 1 s: force it due by moving the timer into the past.
        s.autosave.interval_secs = 1;
        s.autosave.last = Instant::now() - Duration::from_secs(5);
        s.tick_autosave(Instant::now());
        let auto = autosave_path(&main);
        assert!(auto.exists());
        assert!(!with_suffix(&auto, ".tmp").exists(), "temp file renamed away");
        assert_eq!(std::fs::read(&main).unwrap(), before, "main file untouched");
        let restored = Store::open(&auto).unwrap().load().unwrap();
        assert_eq!(restored.zones[0].snapshots[0].points.len(), 1);
        // A manual save supersedes the autosave.
        s.save_to(&main).unwrap();
        assert!(!auto.exists());
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn recovery_is_offered_only_when_newer_and_restores_unsaved_work() {
        let dir = temp_dir();
        let main = dir.join("site.ospot");
        let mut s = Survey::default();
        s.save_to(&main).unwrap();
        assert!(recovery_candidate(&main).is_none(), "no autosave yet");
        s.add_scan(1.0, 1.0, vec![]);
        write_autosave(&s.project, &main).unwrap();
        // Make the autosave strictly newer regardless of filesystem timestamp granularity.
        let later = SystemTime::now() + Duration::from_secs(10);
        std::fs::File::options().write(true).open(autosave_path(&main)).unwrap().set_modified(later).unwrap();
        assert!(recovery_candidate(&main).is_some());

        let mut reopened = Survey::default();
        reopened.open_from(&main).unwrap();
        assert!(reopened.autosave.recovery.is_some(), "opening offers the newer autosave");
        assert!(reopened.snapshot().points.is_empty());
        reopened.recover_autosave().unwrap();
        assert_eq!(reopened.snapshot().points.len(), 1);
        assert!(reopened.dirty);
        assert_eq!(reopened.path.as_deref(), Some(main.as_path()));

        // An older autosave is not offered.
        std::fs::File::options().write(true).open(autosave_path(&main)).unwrap().set_modified(SystemTime::UNIX_EPOCH).unwrap();
        assert!(recovery_candidate(&main).is_none());
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn discard_removes_the_autosave() {
        let dir = temp_dir();
        let main = dir.join("site.ospot");
        let mut s = Survey::default();
        s.save_to(&main).unwrap();
        write_autosave(&s.project, &main).unwrap();
        s.autosave.recovery = Some(autosave_path(&main));
        s.discard_autosave();
        assert!(!autosave_path(&main).exists());
        assert!(s.autosave.recovery.is_none());
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn stale_temp_file_does_not_block_a_new_autosave() {
        let dir = temp_dir();
        let main = dir.join("site.ospot");
        std::fs::write(with_suffix(&autosave_path(&main), ".tmp"), b"garbage from a crash").unwrap();
        let p = Project::new("x");
        let path = write_autosave(&p, &main).unwrap();
        assert_eq!(Store::open(&path).unwrap().load().unwrap().name, "x");
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }
}
