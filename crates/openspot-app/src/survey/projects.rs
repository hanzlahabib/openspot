//! Recent-files list and sortable project metadata rows for the start page. Pure logic;
//! drawing lives in `ui/start.rs`.

use std::path::{Path, PathBuf};

use openspot_store::Store;
use serde::{Deserialize, Serialize};

/// Longest recent list kept: enough to find last week's jobs, short enough to scan.
pub const MAX_RECENT: usize = 12;

/// A project file the user opened or saved, with when that last happened (Unix seconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: String,
    pub last_opened: i64,
}

/// Moves `path` to the front of `list` (dropping an older entry for the same file) and
/// caps the length. Paths that are not valid UTF-8 cannot be stored in the JSON prefs and
/// are skipped.
pub fn push_recent(list: &mut Vec<RecentEntry>, path: &Path, now: i64) {
    let Some(path) = path.to_str() else { return };
    list.retain(|e| e.path != path);
    list.insert(0, RecentEntry { path: path.to_owned(), last_opened: now });
    list.truncate(MAX_RECENT);
}

/// Drops entries whose file no longer exists according to `exists`.
pub fn prune_missing(list: &mut Vec<RecentEntry>, exists: impl Fn(&Path) -> bool) {
    list.retain(|e| exists(Path::new(&e.path)));
}

/// One line of the project list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub path: PathBuf,
    pub name: String,
    pub created: i64,
    pub changed: i64,
    pub last_opened: i64,
    pub zone_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    Name,
    Created,
    Changed,
    #[default]
    LastOpened,
    Zones,
}

impl SortKey {
    pub const ALL: [SortKey; 5] = [Self::Name, Self::Created, Self::Changed, Self::LastOpened, Self::Zones];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Created => "Created",
            Self::Changed => "Changed",
            Self::LastOpened => "Last opened",
            Self::Zones => "Zones",
        }
    }

    /// Direction a column starts in when first clicked: names A-Z, everything else largest
    /// or newest first.
    pub fn starts_ascending(self) -> bool {
        self == Self::Name
    }
}

/// Sorts rows by `key`. Ties fall back to name then path so the order is stable between
/// frames (a flickering list is unusable).
pub fn sort_rows(rows: &mut [ProjectRow], key: SortKey, ascending: bool) {
    rows.sort_by(|a, b| {
        let primary = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Created => a.created.cmp(&b.created),
            SortKey::Changed => a.changed.cmp(&b.changed),
            SortKey::LastOpened => a.last_opened.cmp(&b.last_opened),
            SortKey::Zones => a.zone_count.cmp(&b.zone_count),
        };
        let primary = if ascending { primary } else { primary.reverse() };
        primary.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.path.cmp(&b.path))
    });
}

/// Reads each recent file's metadata. Files that vanished or cannot be read as a project
/// are skipped rather than failing the whole page.
pub fn load_rows(recent: &[RecentEntry]) -> Vec<ProjectRow> {
    recent
        .iter()
        .filter_map(|e| {
            let path = PathBuf::from(&e.path);
            if !path.exists() {
                return None;
            }
            let summary = Store::open(&path).and_then(|s| s.summary()).ok()?;
            Some(ProjectRow {
                path,
                name: summary.name,
                created: summary.created,
                changed: summary.changed,
                last_opened: e.last_opened,
                zone_count: summary.zone_count,
            })
        })
        .collect()
}

/// "YYYY-MM-DD HH:MM" in UTC, or "unknown" for the 0 placeholder. No time-zone database is
/// bundled, so the zone is stated in the column header instead of guessed.
pub fn format_utc(ts: i64) -> String {
    if ts <= 0 {
        return "unknown".to_owned();
    }
    let (days, secs) = (ts.div_euclid(86_400), ts.rem_euclid(86_400));
    // Civil-from-days (proleptic Gregorian), after H. Hinnant's public-domain algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, created: i64, changed: i64, opened: i64, zones: usize) -> ProjectRow {
        ProjectRow { path: PathBuf::from(format!("/p/{name}")), name: name.into(), created, changed, last_opened: opened, zone_count: zones }
    }

    fn names(rows: &[ProjectRow]) -> Vec<&str> {
        rows.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn recent_list_moves_to_front_dedupes_and_caps() {
        let mut list = Vec::new();
        for i in 0..(MAX_RECENT + 5) {
            push_recent(&mut list, Path::new(&format!("/p/{i}.ospot")), i as i64);
        }
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].path, format!("/p/{}.ospot", MAX_RECENT + 4));
        // Reopening an old entry moves it to the front instead of duplicating it.
        let old = list[5].path.clone();
        push_recent(&mut list, Path::new(&old), 999);
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!((list[0].path.as_str(), list[0].last_opened), (old.as_str(), 999));
        assert_eq!(list.iter().filter(|e| e.path == old).count(), 1);
    }

    #[test]
    fn prune_drops_missing_files_only() {
        let dir = std::env::temp_dir().join(format!("openspot-recent-{}", openspot_model::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let present = dir.join("a.ospot");
        std::fs::write(&present, b"x").unwrap();
        let mut list = Vec::new();
        push_recent(&mut list, &dir.join("gone.ospot"), 1);
        push_recent(&mut list, &present, 2);
        prune_missing(&mut list, |p| p.exists());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, present.to_str().unwrap());
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn sorting_by_each_key_and_direction() {
        let mut rows = vec![row("beta", 10, 50, 7, 2), row("Alpha", 30, 20, 9, 5), row("gamma", 20, 40, 8, 1)];
        sort_rows(&mut rows, SortKey::Name, true);
        assert_eq!(names(&rows), ["Alpha", "beta", "gamma"], "case-insensitive");
        sort_rows(&mut rows, SortKey::Created, false);
        assert_eq!(names(&rows), ["Alpha", "gamma", "beta"]);
        sort_rows(&mut rows, SortKey::Changed, false);
        assert_eq!(names(&rows), ["beta", "gamma", "Alpha"]);
        sort_rows(&mut rows, SortKey::LastOpened, true);
        assert_eq!(names(&rows), ["beta", "gamma", "Alpha"]);
        sort_rows(&mut rows, SortKey::Zones, false);
        assert_eq!(names(&rows), ["Alpha", "beta", "gamma"]);
    }

    #[test]
    fn sort_ties_are_broken_by_name() {
        let mut rows = vec![row("b", 0, 0, 0, 1), row("a", 0, 0, 0, 1)];
        sort_rows(&mut rows, SortKey::Zones, false);
        assert_eq!(names(&rows), ["a", "b"]);
    }

    #[test]
    fn utc_formatting_matches_known_instants() {
        assert_eq!(format_utc(0), "unknown");
        assert_eq!(format_utc(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(format_utc(951_782_400 + 3600 * 5 + 60 * 7), "2000-02-29 05:07");
        assert_eq!(format_utc(86_399), "1970-01-01 23:59");
    }

    #[test]
    fn rows_come_from_real_files_and_skip_bad_ones() {
        let dir = std::env::temp_dir().join(format!("openspot-rows-{}", openspot_model::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.ospot");
        let mut project = openspot_model::Project::new("Office");
        project.zones.push(openspot_model::Zone::new("Z2"));
        project.structure.stamp_saved(1_700_000_000);
        Store::open(&good).unwrap().save(&project).unwrap();
        let junk = dir.join("junk.ospot");
        std::fs::write(&junk, b"not sqlite at all, definitely").unwrap();
        let mut recent = Vec::new();
        for p in [&good, &junk, &dir.join("missing.ospot")] {
            push_recent(&mut recent, p, 42);
        }
        let rows = load_rows(&recent);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!((r.name.as_str(), r.zone_count, r.created, r.changed, r.last_opened), ("Office", 2, 1_700_000_000, 1_700_000_000, 42));
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }
}
