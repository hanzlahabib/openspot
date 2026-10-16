//! User-chosen friendly names for access points, keyed by BSSID.
//!
//! Aliases live either app-wide (in the preferences) or per project (inside the project file,
//! so they travel with it). The UI always edits one [`Aliases`] value, the *active* one, and
//! [`active`] / [`commit`] move it to and from the place [`AliasScope`] selects.

use std::collections::BTreeMap;

use openspot_model::ProjectAliases;
use openspot_wlan::MacAddress;
use serde::{Deserialize, Serialize};

use crate::alias_csv::parse_bssid;

/// Where aliases are stored and edited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasScope {
    /// One list for the whole app, kept in the preferences.
    #[default]
    App,
    /// One list per project, kept in the project file.
    Project,
}

impl AliasScope {
    /// Short description used in menus and notices.
    pub fn label(self) -> &'static str {
        match self {
            Self::App => "app-wide",
            Self::Project => "this project",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Aliases {
    by_bssid: BTreeMap<String, String>,
    /// Which store these aliases came from, so menus and notices can say so.
    scope: AliasScope,
}

/// The aliases the UI should show and edit for `scope`.
pub fn active(scope: AliasScope, app: &Aliases, project: &ProjectAliases) -> Aliases {
    match scope {
        AliasScope::App => app.clone(),
        AliasScope::Project => Aliases::from_project(project),
    }
}

/// Stores `edited` (the result of editing [`active`]) back where it came from, leaving the
/// other store untouched. Returns whether the project's data changed, so the caller can mark
/// the project as having unsaved changes.
pub fn commit(scope: AliasScope, edited: Aliases, app: &mut Aliases, project: &mut ProjectAliases) -> bool {
    match scope {
        AliasScope::App => {
            *app = Aliases { scope: AliasScope::App, ..edited };
            false
        }
        AliasScope::Project => {
            let new = edited.to_project();
            let changed = new != *project;
            *project = new;
            changed
        }
    }
}

impl Aliases {
    /// Builds from the persisted map; keys are normalised to the display form.
    pub fn from_map(map: &BTreeMap<String, String>) -> Self {
        Self {
            by_bssid: map
                .iter()
                .filter(|(_, v)| !v.trim().is_empty())
                .map(|(k, v)| (k.to_uppercase(), v.trim().to_owned()))
                .collect(),
            scope: AliasScope::App,
        }
    }

    /// Builds from a project's aliases.
    pub fn from_project(project: &ProjectAliases) -> Self {
        Self {
            by_bssid: project.by_bssid.iter().map(|(b, name)| (b.to_string(), name.clone())).collect(),
            scope: AliasScope::Project,
        }
    }

    /// The project form; entries whose key is not a valid BSSID cannot be stored per project
    /// and are dropped (only hand-edited preference files can contain such keys).
    pub fn to_project(&self) -> ProjectAliases {
        ProjectAliases { by_bssid: self.by_bssid.iter().filter_map(|(k, v)| Some((parse_bssid(k)?, v.clone()))).collect() }
    }

    pub fn scope(&self) -> AliasScope {
        self.scope
    }

    pub fn to_map(&self) -> BTreeMap<String, String> {
        self.by_bssid.clone()
    }

    pub fn get(&self, bssid: &MacAddress) -> Option<&str> {
        self.by_bssid.get(&bssid.to_string()).map(String::as_str)
    }

    /// Sets the alias; blank text removes it.
    pub fn set(&mut self, bssid: &MacAddress, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.by_bssid.remove(&bssid.to_string());
        } else {
            self.by_bssid.insert(bssid.to_string(), name.to_owned());
        }
    }

    pub fn len(&self) -> usize {
        self.by_bssid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_bssid.is_empty()
    }

    /// Every alias as (display-form BSSID, name), ordered by BSSID.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.by_bssid.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn clear(&mut self) {
        self.by_bssid.clear();
    }

    /// Alias if set, else the SSID, else a placeholder for hidden networks.
    pub fn name_for(&self, bssid: &MacAddress, ssid: &str) -> String {
        match self.get(bssid) {
            Some(alias) => alias.to_owned(),
            None if ssid.is_empty() => "<hidden>".to_owned(),
            None => ssid.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AP: MacAddress = MacAddress([0xAA, 0xBB, 0xCC, 0, 0, 1]);

    #[test]
    fn set_get_and_remove() {
        let mut a = Aliases::default();
        assert_eq!(a.get(&AP), None);
        a.set(&AP, "  Office AP  ");
        assert_eq!(a.get(&AP), Some("Office AP"));
        a.set(&AP, "   ");
        assert_eq!(a.get(&AP), None);
    }

    #[test]
    fn name_prefers_alias_then_ssid_then_placeholder() {
        let mut a = Aliases::default();
        assert_eq!(a.name_for(&AP, "Home"), "Home");
        assert_eq!(a.name_for(&AP, ""), "<hidden>");
        a.set(&AP, "Kitchen");
        assert_eq!(a.name_for(&AP, "Home"), "Kitchen");
    }

    #[test]
    fn map_round_trip_normalises_keys_and_drops_blanks() {
        let map = BTreeMap::from([("aa:bb:cc:00:00:01".to_owned(), " Lab ".to_owned()), ("11:22:33:44:55:66".to_owned(), "  ".to_owned())]);
        let a = Aliases::from_map(&map);
        assert_eq!(a.get(&AP), Some("Lab"));
        assert_eq!(a.to_map().len(), 1);
        assert_eq!(Aliases::from_map(&a.to_map()), a);
    }

    #[test]
    fn app_scope_edits_never_touch_the_project() {
        let mut app = Aliases::default();
        app.set(&AP, "Global");
        let mut project = ProjectAliases::default();
        project.by_bssid.insert(AP, "Local".into());
        let mut edited = active(AliasScope::App, &app, &project);
        assert_eq!(edited.get(&AP), Some("Global"));
        edited.set(&AP, "Global 2");
        assert!(!commit(AliasScope::App, edited, &mut app, &mut project), "the project is not dirtied");
        assert_eq!(app.get(&AP), Some("Global 2"));
        assert_eq!(project.by_bssid[&AP], "Local");
    }

    #[test]
    fn project_scope_edits_never_touch_the_app_list() {
        let mut app = Aliases::default();
        app.set(&AP, "Global");
        let mut project = ProjectAliases::default();
        let mut edited = active(AliasScope::Project, &app, &project);
        assert_eq!((edited.get(&AP), edited.scope()), (None, AliasScope::Project), "app-wide names are not shown");
        edited.set(&AP, "Local");
        assert!(commit(AliasScope::Project, edited, &mut app, &mut project));
        assert_eq!(project.by_bssid[&AP], "Local");
        assert_eq!(app.get(&AP), Some("Global"));
        assert_eq!(app.scope(), AliasScope::App);
    }

    #[test]
    fn committing_unchanged_project_aliases_does_not_report_a_change() {
        let mut project = ProjectAliases::default();
        project.by_bssid.insert(AP, "Local".into());
        let edited = active(AliasScope::Project, &Aliases::default(), &project);
        assert!(!commit(AliasScope::Project, edited, &mut Aliases::default(), &mut project));
        let mut cleared = active(AliasScope::Project, &Aliases::default(), &project);
        cleared.clear();
        assert!(commit(AliasScope::Project, cleared, &mut Aliases::default(), &mut project));
        assert!(project.is_empty());
    }

    #[test]
    fn project_conversion_round_trips_and_drops_unparsable_keys() {
        let map = BTreeMap::from([
            ("AA:BB:CC:00:00:01".to_owned(), "Lab".to_owned()),
            ("not a mac".to_owned(), "Junk".to_owned()),
        ]);
        let project = Aliases::from_map(&map).to_project();
        assert_eq!(project.len(), 1);
        assert_eq!(Aliases::from_project(&project).get(&AP), Some("Lab"));
        assert_eq!(Aliases::from_project(&project).to_project(), project);
    }

    #[test]
    fn scope_serialises_as_a_stable_word_and_defaults_to_app_wide() {
        assert_eq!(serde_json::to_string(&AliasScope::Project).unwrap(), "\"project\"");
        assert_eq!(serde_json::from_str::<AliasScope>("\"app\"").unwrap(), AliasScope::App);
        assert_eq!(AliasScope::default(), AliasScope::App);
    }
}
