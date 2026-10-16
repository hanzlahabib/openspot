//! Hidden networks (empty SSID) in a snapshot: listing them and filtering them out.

use std::borrow::Cow;
use std::collections::BTreeSet;

use openspot_wlan::MacAddress;

use crate::Snapshot;

/// A BSSID that was seen with an empty SSID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiddenSighting {
    pub bssid: MacAddress,
    /// Strongest RSSI seen while hidden.
    pub best_rssi_dbm: i32,
    /// Measurement points that saw it.
    pub points: usize,
}

/// Hidden BSSIDs of `snapshot`, strongest first (ties by address).
pub fn hidden_networks(snapshot: &Snapshot) -> Vec<HiddenSighting> {
    let mut found: std::collections::BTreeMap<MacAddress, HiddenSighting> = std::collections::BTreeMap::new();
    for point in &snapshot.points {
        for ap in point.access_points.iter().filter(|a| a.ssid.is_empty()) {
            let e = found.entry(ap.bssid).or_insert(HiddenSighting { bssid: ap.bssid, best_rssi_dbm: ap.rssi_dbm, points: 0 });
            e.best_rssi_dbm = e.best_rssi_dbm.max(ap.rssi_dbm);
            e.points += 1;
        }
    }
    let mut v: Vec<_> = found.into_values().collect();
    v.sort_by(|a, b| b.best_rssi_dbm.cmp(&a.best_rssi_dbm).then(a.bssid.cmp(&b.bssid)));
    v
}

/// `snapshot` without the readings of `excluded` BSSIDs that were hidden (empty SSID).
/// Borrowed unchanged when there is nothing to remove. Measurement points are kept even if
/// they end up with no readings, so positions and counts stay consistent.
pub fn without_hidden<'a>(snapshot: &'a Snapshot, excluded: &BTreeSet<MacAddress>) -> Cow<'a, Snapshot> {
    let drops = |ap: &openspot_wlan::AccessPoint| ap.ssid.is_empty() && excluded.contains(&ap.bssid);
    if excluded.is_empty() || !snapshot.points.iter().flat_map(|p| &p.access_points).any(drops) {
        return Cow::Borrowed(snapshot);
    }
    let mut copy = snapshot.clone();
    for p in &mut copy.points {
        p.access_points.retain(|a| !drops(a));
    }
    Cow::Owned(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScanPoint;
    use openspot_wlan::AccessPoint;

    fn ap(last: u8, ssid: &str, rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
    }

    fn snapshot() -> Snapshot {
        let mut s = Snapshot::new("s");
        s.points.push(ScanPoint::new(0.0, 0.0, 0, vec![ap(1, "", -70), ap(2, "visible", -40), ap(3, "", -60)]));
        s.points.push(ScanPoint::new(1.0, 0.0, 0, vec![ap(1, "", -55), ap(2, "visible", -45)]));
        s
    }

    #[test]
    fn lists_only_hidden_bssids_strongest_first_with_counts() {
        let h = hidden_networks(&snapshot());
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].bssid.0[5], h[0].best_rssi_dbm, h[0].points), (1, -55, 2));
        assert_eq!((h[1].bssid.0[5], h[1].best_rssi_dbm, h[1].points), (3, -60, 1));
    }

    #[test]
    fn excluding_removes_only_those_hidden_readings() {
        let s = snapshot();
        let ex = BTreeSet::from([MacAddress([0, 0, 0, 0, 0, 1])]);
        let f = without_hidden(&s, &ex);
        assert_eq!(f.points.len(), 2, "points are kept");
        assert_eq!(f.points[0].access_points.len(), 2);
        assert!(f.points[0].rssi_of(&MacAddress([0, 0, 0, 0, 0, 1])).is_none());
        assert!(f.points[0].rssi_of(&MacAddress([0, 0, 0, 0, 0, 3])).is_some());
        assert!(f.points[1].rssi_of(&MacAddress([0, 0, 0, 0, 0, 2])).is_some());
        // The source is untouched.
        assert_eq!(s.points[0].access_points.len(), 3);
    }

    #[test]
    fn a_named_network_is_never_removed_even_if_its_bssid_is_listed() {
        let ex = BTreeSet::from([MacAddress([0, 0, 0, 0, 0, 2])]);
        let s = snapshot();
        assert!(matches!(without_hidden(&s, &ex), Cow::Borrowed(_)));
    }

    #[test]
    fn nothing_excluded_borrows() {
        let s = snapshot();
        assert!(matches!(without_hidden(&s, &BTreeSet::new()), Cow::Borrowed(_)));
    }
}
