//! Custom AP groups applied to the Survey network list: partitioning, group filter and
//! whole-group selection. Pure logic; the egui list lives in `ui/side.rs`.

use std::collections::BTreeSet;

use openspot_wlan::MacAddress;

use crate::groups::CustomGroups;

/// One entry of `Snapshot::networks()`: BSSID, SSID of the strongest sighting, best RSSI.
pub type NetworkEntry = (MacAddress, String, i32);

/// Which part of the network list is shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GroupFilter {
    #[default]
    All,
    /// Only the members of the group with this name (case-insensitive, like group names).
    Group(String),
    /// Only networks that belong to no group.
    Ungrouped,
}

impl GroupFilter {
    /// A filter naming a group that was since deleted or renamed would show an empty list
    /// the user cannot explain, so it falls back to showing everything.
    pub fn resolved(&self, groups: &CustomGroups) -> Self {
        match self {
            Self::Group(name) if !groups.iter().any(|g| g.name.eq_ignore_ascii_case(name)) => Self::All,
            other => other.clone(),
        }
    }
}

/// Networks under one header. `name` is `None` for the networks without a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: Option<String>,
    pub rows: Vec<NetworkEntry>,
}

/// Splits `nets` into sections in group-definition order with the ungrouped networks last,
/// keeping the incoming (strongest first) order inside each section. Empty sections are
/// dropped, as are sections excluded by `filter`.
pub fn sections(nets: &[NetworkEntry], groups: &CustomGroups, filter: &GroupFilter) -> Vec<Section> {
    let mut out: Vec<Section> = groups.iter().map(|g| Section { name: Some(g.name.clone()), rows: Vec::new() }).collect();
    let mut ungrouped = Section { name: None, rows: Vec::new() };
    for net in nets {
        match groups.group_of(&net.0).and_then(|g| out.iter_mut().find(|s| s.name.as_deref() == Some(g))) {
            Some(section) => section.rows.push(net.clone()),
            None => ungrouped.rows.push(net.clone()),
        }
    }
    out.push(ungrouped);
    out.retain(|s| {
        !s.rows.is_empty()
            && match filter {
                GroupFilter::All => true,
                GroupFilter::Group(name) => s.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)),
                GroupFilter::Ungrouped => s.name.is_none(),
            }
    });
    out
}

/// How much of a section is ticked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ticked {
    None,
    Some,
    All,
}

pub fn ticked(selected: &BTreeSet<MacAddress>, section: &Section) -> Ticked {
    let n = section.rows.iter().filter(|r| selected.contains(&r.0)).count();
    match n {
        0 => Ticked::None,
        n if n == section.rows.len() => Ticked::All,
        _ => Ticked::Some,
    }
}

/// Ticks or unticks every network in `section`; other selections are untouched.
pub fn set_section(selected: &mut BTreeSet<MacAddress>, section: &Section, on: bool) {
    for row in &section.rows {
        if on {
            selected.insert(row.0);
        } else {
            selected.remove(&row.0);
        }
    }
}

/// The "All" button: ticks everything that is currently visible.
pub fn select_visible(selected: &mut BTreeSet<MacAddress>, visible: &[Section]) {
    for section in visible {
        set_section(selected, section, true);
    }
}

/// The "None" button. Unfiltered it clears the whole selection (including networks no longer
/// in the list); filtered it unticks only what is visible, so a selection built from several
/// groups survives clearing one of them.
pub fn deselect_visible(selected: &mut BTreeSet<MacAddress>, visible: &[Section], filter: &GroupFilter) {
    if *filter == GroupFilter::All {
        selected.clear();
    } else {
        for section in visible {
            set_section(selected, section, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(n: u8) -> MacAddress {
        MacAddress([0xAA, 0, 0, 0, 0, n])
    }

    fn net(n: u8, rssi: i32) -> NetworkEntry {
        (mac(n), format!("net{n}"), rssi)
    }

    /// Lobby = {1, 3}, Lab = {2}; 4 and 5 are ungrouped.
    fn fixture() -> (Vec<NetworkEntry>, CustomGroups) {
        let mut g = CustomGroups::default();
        g.create("Lobby").unwrap();
        g.create("Lab").unwrap();
        for (n, group) in [(1, "Lobby"), (3, "Lobby"), (2, "Lab")] {
            g.assign(&mac(n), Some(group)).unwrap();
        }
        // Strongest first, as Snapshot::networks() returns them.
        (vec![net(4, -40), net(1, -50), net(2, -55), net(3, -60), net(5, -70)], g)
    }

    fn macs(s: &Section) -> Vec<MacAddress> {
        s.rows.iter().map(|r| r.0).collect()
    }

    #[test]
    fn partition_follows_group_order_with_ungrouped_last_and_keeps_signal_order() {
        let (nets, groups) = fixture();
        let s = sections(&nets, &groups, &GroupFilter::All);
        let names: Vec<_> = s.iter().map(|x| x.name.as_deref()).collect();
        assert_eq!(names, [Some("Lobby"), Some("Lab"), None]);
        assert_eq!(macs(&s[0]), [mac(1), mac(3)]);
        assert_eq!(macs(&s[1]), [mac(2)]);
        assert_eq!(macs(&s[2]), [mac(4), mac(5)]);
    }

    #[test]
    fn every_network_appears_exactly_once() {
        let (nets, groups) = fixture();
        let s = sections(&nets, &groups, &GroupFilter::All);
        let mut all: Vec<_> = s.iter().flat_map(macs).collect();
        all.sort();
        assert_eq!(all, (1..=5).map(mac).collect::<Vec<_>>());
    }

    #[test]
    fn groups_without_measured_members_and_empty_ungrouped_are_dropped() {
        let (nets, mut groups) = fixture();
        groups.create("Garage").unwrap();
        let only_grouped: Vec<_> = nets.iter().filter(|n| n.0 != mac(4) && n.0 != mac(5)).cloned().collect();
        let s = sections(&only_grouped, &groups, &GroupFilter::All);
        assert_eq!(s.iter().map(|x| x.name.as_deref()).collect::<Vec<_>>(), [Some("Lobby"), Some("Lab")]);
    }

    #[test]
    fn filter_shows_one_group_or_the_ungrouped_networks() {
        let (nets, groups) = fixture();
        let lobby = sections(&nets, &groups, &GroupFilter::Group("lobby".into()));
        assert_eq!(lobby.len(), 1);
        assert_eq!(macs(&lobby[0]), [mac(1), mac(3)]);
        let rest = sections(&nets, &groups, &GroupFilter::Ungrouped);
        assert_eq!(rest.len(), 1);
        assert_eq!(macs(&rest[0]), [mac(4), mac(5)]);
        assert!(sections(&nets, &groups, &GroupFilter::Group("Missing".into())).is_empty());
    }

    #[test]
    fn filter_naming_a_deleted_group_falls_back_to_all() {
        let (_, mut groups) = fixture();
        let f = GroupFilter::Group("Lab".into());
        assert_eq!(f.resolved(&groups), f);
        groups.delete("Lab").unwrap();
        assert_eq!(f.resolved(&groups), GroupFilter::All);
        assert_eq!(GroupFilter::Ungrouped.resolved(&groups), GroupFilter::Ungrouped);
    }

    #[test]
    fn ticked_state_is_none_some_or_all() {
        let (nets, groups) = fixture();
        let lobby = sections(&nets, &groups, &GroupFilter::All).remove(0);
        let mut sel = BTreeSet::new();
        assert_eq!(ticked(&sel, &lobby), Ticked::None);
        sel.insert(mac(1));
        assert_eq!(ticked(&sel, &lobby), Ticked::Some);
        sel.insert(mac(3));
        assert_eq!(ticked(&sel, &lobby), Ticked::All);
        sel.insert(mac(2));
        assert_eq!(ticked(&sel, &lobby), Ticked::All, "members of other groups do not count");
    }

    #[test]
    fn selecting_a_group_ticks_only_its_members_and_can_be_undone() {
        let (nets, groups) = fixture();
        let s = sections(&nets, &groups, &GroupFilter::All);
        let mut sel = BTreeSet::from([mac(5)]);
        set_section(&mut sel, &s[0], true);
        assert_eq!(sel, BTreeSet::from([mac(1), mac(3), mac(5)]));
        set_section(&mut sel, &s[0], false);
        assert_eq!(sel, BTreeSet::from([mac(5)]), "other selections are kept");
    }

    #[test]
    fn all_and_none_respect_the_filter() {
        let (nets, groups) = fixture();
        let filter = GroupFilter::Group("Lobby".into());
        let visible = sections(&nets, &groups, &filter);
        let mut sel = BTreeSet::from([mac(2)]);
        select_visible(&mut sel, &visible);
        assert_eq!(sel, BTreeSet::from([mac(1), mac(2), mac(3)]), "hidden selections are kept, visible ones added");
        deselect_visible(&mut sel, &visible, &filter);
        assert_eq!(sel, BTreeSet::from([mac(2)]), "only the visible ones are cleared");

        let all = sections(&nets, &groups, &GroupFilter::All);
        sel.insert(mac(99));
        deselect_visible(&mut sel, &all, &GroupFilter::All);
        assert!(sel.is_empty(), "unfiltered None clears everything, as before groups existed");
    }
}
