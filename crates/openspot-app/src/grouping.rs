//! Group-by for the Inspector table: bucketing rows and flattening buckets into table entries.

use std::collections::{BTreeMap, BTreeSet};

use openspot_wlan::MacAddress;

use crate::columns::vendor_label;
use crate::state::Track;

/// Label of the bucket holding rows that have no value for the grouping attribute.
pub const UNGROUPED_LABEL: &str = "Ungrouped";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GroupBy {
    #[default]
    None,
    Ssid,
    Channel,
    Band,
    Security,
    Vendor,
    Custom,
}

impl GroupBy {
    pub const ALL: [Self; 7] = [Self::None, Self::Ssid, Self::Channel, Self::Band, Self::Security, Self::Vendor, Self::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No grouping",
            Self::Ssid => "SSID",
            Self::Channel => "Channel",
            Self::Band => "Band",
            Self::Security => "Security",
            Self::Vendor => "Vendor",
            Self::Custom => "Custom group",
        }
    }

    /// Stable preference key; never change an existing one.
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ssid => "ssid",
            Self::Channel => "channel",
            Self::Band => "band",
            Self::Security => "security",
            Self::Vendor => "vendor",
            Self::Custom => "custom",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.key() == key)
    }
}

/// Bucket of rows sharing one attribute value. `label` is `None` for the ungrouped bucket.
#[derive(Debug)]
pub struct Group<'a> {
    pub label: Option<String>,
    pub rows: Vec<&'a Track>,
}

impl Group<'_> {
    pub fn title(&self) -> &str {
        self.label.as_deref().unwrap_or(UNGROUPED_LABEL)
    }
}

/// Sorts groups by value (numerically for channels, in band order for bands), not alphabetically.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum OrderKey {
    Number(u32),
    Text(String),
}

/// Bucket and display label of `track` under `by`; `None` means "has no value" (ungrouped).
/// `custom_group` supplies the user's group name for a BSSID.
fn bucket_of(track: &Track, by: GroupBy, custom_group: &impl Fn(&MacAddress) -> Option<String>) -> Option<(OrderKey, String)> {
    let ap = &track.ap;
    let text = |s: String| if s.is_empty() { None } else { Some((OrderKey::Text(s.to_lowercase()), s)) };
    match by {
        GroupBy::None => None,
        GroupBy::Ssid => text(ap.ssid.clone()),
        GroupBy::Channel => (ap.channel != 0).then(|| (OrderKey::Number(u32::from(ap.channel)), format!("Channel {}", ap.channel))),
        GroupBy::Band => ap.band.map(|b| (OrderKey::Number(b as u32), b.label().to_owned())),
        GroupBy::Security => text(ap.security.iter().map(|s| s.label()).collect::<Vec<_>>().join(" + ")),
        GroupBy::Vendor => ap.bssid.vendor().is_some().then(|| vendor_label(ap).to_owned()).and_then(text),
        GroupBy::Custom => custom_group(&ap.bssid).and_then(text),
    }
}

/// Splits `rows` (already filtered and sorted) into groups, keeping the row order inside each.
/// Groups are ordered by value with the ungrouped bucket last; it is omitted when empty.
/// With [`GroupBy::None`] there is a single ungrouped group holding every row.
pub fn group_rows<'a>(rows: &[&'a Track], by: GroupBy, custom_group: impl Fn(&MacAddress) -> Option<String>) -> Vec<Group<'a>> {
    let mut buckets: BTreeMap<OrderKey, (String, Vec<&'a Track>)> = BTreeMap::new();
    let mut ungrouped = Vec::new();
    for &track in rows {
        match bucket_of(track, by, &custom_group) {
            Some((order, label)) => buckets.entry(order).or_insert_with(|| (label, Vec::new())).1.push(track),
            None => ungrouped.push(track),
        }
    }
    let mut groups: Vec<Group<'a>> = buckets.into_values().map(|(label, rows)| Group { label: Some(label), rows }).collect();
    if !ungrouped.is_empty() {
        groups.push(Group { label: None, rows: ungrouped });
    }
    groups
}

/// One line of the grouped table.
#[derive(Debug)]
pub enum Entry<'a> {
    Header { title: String, count: usize, collapsed: bool },
    Row(&'a Track),
}

/// Flattens groups into table lines. With no grouping there are no headers. Collapsed groups
/// keep their header but hide their rows; collapse state is keyed by group title.
pub fn flatten<'a>(groups: &[Group<'a>], by: GroupBy, collapsed: &BTreeSet<String>) -> Vec<Entry<'a>> {
    if by == GroupBy::None {
        return groups.iter().flat_map(|g| g.rows.iter().map(|&t| Entry::Row(t))).collect();
    }
    let mut out = Vec::new();
    for g in groups {
        let is_collapsed = collapsed.contains(g.title());
        out.push(Entry::Header { title: g.title().to_owned(), count: g.rows.len(), collapsed: is_collapsed });
        if !is_collapsed {
            out.extend(g.rows.iter().map(|&t| Entry::Row(t)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Tracker;
    use openspot_wlan::AccessPoint;

    fn ap(last: u8, ssid: &str, rssi: i32, khz: u32) -> AccessPoint {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, khz, true, 100, vec![])
    }

    fn tracker(aps: Vec<AccessPoint>) -> Tracker {
        let mut t = Tracker::default();
        t.ingest(aps, 0.0);
        t
    }

    fn titles(groups: &[Group<'_>]) -> Vec<(String, usize)> {
        groups.iter().map(|g| (g.title().to_owned(), g.rows.len())).collect()
    }

    fn no_custom(_: &MacAddress) -> Option<String> {
        None
    }

    #[test]
    fn groups_by_ssid_with_hidden_networks_ungrouped() {
        let t = tracker(vec![ap(1, "Home", -50, 2_437_000), ap(2, "home", -60, 2_437_000), ap(3, "", -70, 2_437_000), ap(4, "Zed", -40, 5_180_000)]);
        let rows = t.view(&Default::default(), Default::default());
        let groups = group_rows(&rows, GroupBy::Ssid, no_custom);
        // Group order is case-insensitive alphabetical; label keeps the first spelling seen in row order.
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].rows.len(), 2);
        assert_eq!(groups[1].title(), "Zed");
        assert_eq!(groups[2].label, None);
        assert_eq!(groups[2].title(), UNGROUPED_LABEL);
        assert_eq!(groups[2].rows[0].ap.bssid, MacAddress([0, 0, 0, 0, 0, 3]));
    }

    #[test]
    fn channels_sort_numerically_not_lexically() {
        let t = tracker(vec![ap(1, "a", -50, 2_412_000), ap(2, "b", -50, 5_180_000), ap(3, "c", -50, 2_437_000), ap(4, "d", -50, 2_437_000)]);
        let rows = t.view(&Default::default(), Default::default());
        let g = group_rows(&rows, GroupBy::Channel, no_custom);
        assert_eq!(titles(&g), vec![("Channel 1".into(), 1), ("Channel 6".into(), 2), ("Channel 36".into(), 1)]);
    }

    #[test]
    fn band_groups_follow_band_order() {
        let t = tracker(vec![ap(1, "a", -50, 5_180_000), ap(2, "b", -50, 2_437_000)]);
        let rows = t.view(&Default::default(), Default::default());
        let g = group_rows(&rows, GroupBy::Band, no_custom);
        assert_eq!(titles(&g), vec![("2.4 GHz".into(), 1), ("5 GHz".into(), 1)]);
    }

    #[test]
    fn vendor_grouping_puts_unknown_vendors_in_ungrouped() {
        let vm = AccessPoint::from_beacon("vm".into(), MacAddress([0x00, 0x50, 0x56, 0, 0, 1]), -50, 2_437_000, true, 100, vec![]);
        // Locally administered (private) addresses have no registered vendor.
        let private = AccessPoint::from_beacon("x".into(), MacAddress([0x02, 0, 0, 0, 0, 2]), -60, 2_437_000, true, 100, vec![]);
        let t = tracker(vec![vm, private]);
        let rows = t.view(&Default::default(), Default::default());
        let g = group_rows(&rows, GroupBy::Vendor, no_custom);
        assert_eq!(g.len(), 2);
        assert!(g[0].title().to_lowercase().contains("vmware"));
        assert_eq!(g[1].label, None);
    }

    #[test]
    fn security_groups_use_the_combined_label() {
        let t = tracker(vec![ap(1, "a", -50, 2_437_000)]);
        let rows = t.view(&Default::default(), Default::default());
        let g = group_rows(&rows, GroupBy::Security, no_custom);
        assert_eq!(g.len(), 1);
        assert!(g[0].label.is_some(), "a secured AP with no IEs still has a security label");
    }

    #[test]
    fn custom_groups_use_the_supplied_lookup_and_rest_is_ungrouped() {
        let t = tracker(vec![ap(1, "a", -50, 2_437_000), ap(2, "b", -50, 2_437_000)]);
        let rows = t.view(&Default::default(), Default::default());
        let lookup = |m: &MacAddress| (m.0[5] == 1).then(|| "Lobby".to_owned());
        let g = group_rows(&rows, GroupBy::Custom, lookup);
        assert_eq!(titles(&g), vec![("Lobby".into(), 1), (UNGROUPED_LABEL.into(), 1)]);
    }

    #[test]
    fn no_grouping_is_one_ungrouped_group_with_row_order_kept() {
        let t = tracker(vec![ap(1, "a", -70, 2_437_000), ap(2, "b", -40, 2_437_000)]);
        let rows = t.view(&Default::default(), Default::default());
        let g = group_rows(&rows, GroupBy::None, no_custom);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].rows[0].ap.ssid, "b");
        let flat = flatten(&g, GroupBy::None, &BTreeSet::new());
        assert!(flat.iter().all(|e| matches!(e, Entry::Row(_))));
        assert_eq!(flat.len(), 2);
    }

    #[test]
    fn empty_input_gives_no_groups() {
        assert!(group_rows(&[], GroupBy::Ssid, no_custom).is_empty());
    }

    #[test]
    fn collapsed_groups_keep_header_and_hide_rows() {
        let t = tracker(vec![ap(1, "a", -50, 2_437_000), ap(2, "b", -50, 5_180_000), ap(3, "c", -50, 5_180_000)]);
        let rows = t.view(&Default::default(), Default::default());
        let g = group_rows(&rows, GroupBy::Band, no_custom);
        let collapsed = BTreeSet::from(["5 GHz".to_owned()]);
        let flat = flatten(&g, GroupBy::Band, &collapsed);
        // 2.4 header + 1 row, 5 GHz header only.
        assert_eq!(flat.len(), 3);
        assert!(matches!(&flat[2], Entry::Header { title, count: 2, collapsed: true } if title == "5 GHz"));
        let open = flatten(&g, GroupBy::Band, &BTreeSet::new());
        assert_eq!(open.len(), 5);
    }

    #[test]
    fn keys_round_trip() {
        for g in GroupBy::ALL {
            assert_eq!(GroupBy::from_key(g.key()), Some(g));
        }
        assert_eq!(GroupBy::from_key("???"), None);
    }
}
