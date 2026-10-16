//! User-defined access point groups. An AP belongs to at most one group, so "group by custom
//! group" has a single answer per row.

use std::collections::BTreeSet;

use openspot_wlan::MacAddress;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::grouping::UNGROUPED_LABEL;

/// Longest group name in characters; keeps table group headers readable.
pub const MAX_NAME_CHARS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomGroup {
    pub name: String,
    /// BSSIDs in display form (`AA:BB:CC:DD:EE:FF`), like the alias map.
    pub members: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GroupError {
    #[error("the group name is empty")]
    Empty,
    #[error("the group name is longer than {MAX_NAME_CHARS} characters")]
    TooLong,
    #[error("a group with this name already exists")]
    Duplicate,
    #[error("\"{UNGROUPED_LABEL}\" is reserved for networks without a group")]
    Reserved,
    #[error("no such group")]
    NotFound,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomGroups {
    groups: Vec<CustomGroup>,
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl CustomGroups {
    /// Rebuilds from persisted data, dropping anything that would break the invariants
    /// (blank, reserved or repeated names; an AP listed in two groups keeps the first).
    pub fn from_saved(saved: &[CustomGroup]) -> Self {
        let mut out = Self::default();
        for g in saved {
            let Ok(name) = validate_name(&g.name) else { continue };
            if out.position(&name).is_some() {
                continue;
            }
            let members = g.members.iter().map(|m| m.to_uppercase()).filter(|m| out.group_of_key(m).is_none()).collect();
            out.groups.push(CustomGroup { name, members });
        }
        out
    }

    pub fn to_saved(&self) -> Vec<CustomGroup> {
        self.groups.clone()
    }

    pub fn iter(&self) -> impl Iterator<Item = &CustomGroup> {
        self.groups.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.groups.iter().position(|g| same_name(&g.name, name))
    }

    fn group_of_key(&self, key: &str) -> Option<&str> {
        self.groups.iter().find(|g| g.members.contains(key)).map(|g| g.name.as_str())
    }

    pub fn group_of(&self, bssid: &MacAddress) -> Option<&str> {
        self.group_of_key(&bssid.to_string())
    }

    /// Adds an empty group and returns its normalised name.
    pub fn create(&mut self, name: &str) -> Result<String, GroupError> {
        let name = validate_name(name)?;
        if self.position(&name).is_some() {
            return Err(GroupError::Duplicate);
        }
        self.groups.push(CustomGroup { name: name.clone(), members: BTreeSet::new() });
        Ok(name)
    }

    /// Renames a group. Changing only the letter case of its own name is allowed.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), GroupError> {
        let new = validate_name(new)?;
        let at = self.position(old).ok_or(GroupError::NotFound)?;
        if self.position(&new).is_some_and(|other| other != at) {
            return Err(GroupError::Duplicate);
        }
        self.groups[at].name = new;
        Ok(())
    }

    /// Deletes a group; its members become ungrouped.
    pub fn delete(&mut self, name: &str) -> Result<(), GroupError> {
        let at = self.position(name).ok_or(GroupError::NotFound)?;
        self.groups.remove(at);
        Ok(())
    }

    /// Puts `bssid` in `group` (moving it out of any other group), or ungroups it for `None`.
    pub fn assign(&mut self, bssid: &MacAddress, group: Option<&str>) -> Result<(), GroupError> {
        let target = match group {
            Some(name) => Some(self.position(name).ok_or(GroupError::NotFound)?),
            None => None,
        };
        let key = bssid.to_string();
        for g in &mut self.groups {
            g.members.remove(&key);
        }
        if let Some(at) = target {
            self.groups[at].members.insert(key);
        }
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<String, GroupError> {
    let name = name.trim();
    if name.is_empty() {
        Err(GroupError::Empty)
    } else if name.chars().count() > MAX_NAME_CHARS {
        Err(GroupError::TooLong)
    } else if same_name(name, UNGROUPED_LABEL) {
        Err(GroupError::Reserved)
    } else {
        Ok(name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: MacAddress = MacAddress([0xAA, 0, 0, 0, 0, 1]);
    const B: MacAddress = MacAddress([0xAA, 0, 0, 0, 0, 2]);

    #[test]
    fn create_validates_names() {
        let mut g = CustomGroups::default();
        assert_eq!(g.create("  Lobby  "), Ok("Lobby".to_owned()));
        assert_eq!(g.create("lobby"), Err(GroupError::Duplicate));
        assert_eq!(g.create("   "), Err(GroupError::Empty));
        assert_eq!(g.create("ungrouped"), Err(GroupError::Reserved));
        assert_eq!(g.create(&"x".repeat(MAX_NAME_CHARS + 1)), Err(GroupError::TooLong));
        assert!(g.create(&"x".repeat(MAX_NAME_CHARS)).is_ok());
        assert_eq!(g.iter().count(), 2);
    }

    #[test]
    fn assign_moves_between_groups_and_ungroups() {
        let mut g = CustomGroups::default();
        g.create("One").unwrap();
        g.create("Two").unwrap();
        g.assign(&A, Some("one")).unwrap();
        assert_eq!(g.group_of(&A), Some("One"));
        g.assign(&A, Some("Two")).unwrap();
        assert_eq!(g.group_of(&A), Some("Two"));
        assert!(g.iter().find(|x| x.name == "One").unwrap().members.is_empty(), "no longer a member of the old group");
        g.assign(&A, None).unwrap();
        assert_eq!(g.group_of(&A), None);
        assert_eq!(g.assign(&A, Some("Missing")), Err(GroupError::NotFound));
    }

    #[test]
    fn failed_assignment_leaves_membership_untouched() {
        let mut g = CustomGroups::default();
        g.create("One").unwrap();
        g.assign(&A, Some("One")).unwrap();
        assert!(g.assign(&A, Some("Nope")).is_err());
        assert_eq!(g.group_of(&A), Some("One"));
    }

    #[test]
    fn rename_keeps_members_and_rejects_clashes() {
        let mut g = CustomGroups::default();
        g.create("One").unwrap();
        g.create("Two").unwrap();
        g.assign(&A, Some("One")).unwrap();
        g.rename("One", "Uno").unwrap();
        assert_eq!(g.group_of(&A), Some("Uno"));
        assert_eq!(g.rename("Uno", "two"), Err(GroupError::Duplicate));
        g.rename("Uno", "UNO").unwrap();
        assert_eq!(g.group_of(&A), Some("UNO"));
        assert_eq!(g.rename("Missing", "X"), Err(GroupError::NotFound));
        assert_eq!(g.rename("Two", ""), Err(GroupError::Empty));
    }

    #[test]
    fn delete_ungroups_members() {
        let mut g = CustomGroups::default();
        g.create("One").unwrap();
        g.assign(&A, Some("One")).unwrap();
        g.assign(&B, Some("One")).unwrap();
        g.delete("one").unwrap();
        assert_eq!((g.group_of(&A), g.group_of(&B)), (None, None));
        assert_eq!(g.delete("One"), Err(GroupError::NotFound));
    }

    #[test]
    fn saved_data_round_trips_through_json() {
        let mut g = CustomGroups::default();
        g.create("Lobby").unwrap();
        g.assign(&A, Some("Lobby")).unwrap();
        let json = serde_json::to_string(&g.to_saved()).unwrap();
        let back = CustomGroups::from_saved(&serde_json::from_str::<Vec<CustomGroup>>(&json).unwrap());
        assert_eq!(back, g);
    }

    #[test]
    fn corrupt_saved_data_is_repaired_not_trusted() {
        let saved = vec![
            CustomGroup { name: "A".into(), members: BTreeSet::from(["aa:00:00:00:00:01".to_owned()]) },
            CustomGroup { name: "a".into(), members: BTreeSet::new() },
            CustomGroup { name: " ".into(), members: BTreeSet::new() },
            CustomGroup { name: "Ungrouped".into(), members: BTreeSet::new() },
            CustomGroup { name: "B".into(), members: BTreeSet::from(["AA:00:00:00:00:01".to_owned(), "AA:00:00:00:00:02".to_owned()]) },
        ];
        let g = CustomGroups::from_saved(&saved);
        assert_eq!(g.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
        assert_eq!(g.group_of(&A), Some("A"), "lower-case member keys are normalised and the first group wins");
        assert_eq!(g.group_of(&B), Some("B"));
    }
}
