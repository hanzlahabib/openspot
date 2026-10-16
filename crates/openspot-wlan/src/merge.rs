use std::collections::BTreeMap;

use crate::model::{AccessPoint, MacAddress};

/// Merges several probes of the same scan into one list, one entry per BSSID.
///
/// RSSI is the mean of the probes that saw the AP (in dBm, rounded); other fields
/// come from the most recent sighting, which has the freshest IEs.
pub fn merge_probes(probes: impl IntoIterator<Item = Vec<AccessPoint>>) -> Vec<AccessPoint> {
    let mut by_bssid: BTreeMap<MacAddress, (AccessPoint, i64, i64)> = BTreeMap::new();
    for ap in probes.into_iter().flatten() {
        let rssi = i64::from(ap.rssi_dbm);
        by_bssid
            .entry(ap.bssid)
            .and_modify(|(latest, sum, n)| {
                *sum += rssi;
                *n += 1;
                // A probe response can reveal the name of a hidden AP; a later beacon-only
                // sighting must not erase it.
                let known_name =
                    (ap.ssid.is_empty() && !latest.ssid.is_empty()).then(|| latest.ssid.clone());
                let was_hidden = latest.hidden;
                *latest = ap.clone();
                latest.hidden |= was_hidden;
                if let Some(name) = known_name {
                    latest.ssid = name;
                }
            })
            .or_insert((ap, rssi, 1));
    }
    let mut merged: Vec<AccessPoint> = by_bssid
        .into_values()
        .map(|(mut ap, sum, n)| {
            ap.rssi_dbm = (sum as f64 / n as f64).round() as i32;
            ap
        })
        .collect();
    merged.sort_by(|a, b| b.rssi_dbm.cmp(&a.rssi_dbm).then(a.bssid.cmp(&b.bssid)));
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(last: u8, rssi: i32, ssid: &str) -> AccessPoint {
        AccessPoint::from_beacon(
            ssid.into(),
            MacAddress([0, 0, 0, 0, 0, last]),
            rssi,
            2_437_000,
            true,
            100,
            vec![],
        )
    }

    #[test]
    fn averages_rssi_and_keeps_latest_fields() {
        let merged = merge_probes([
            vec![ap(1, -60, "old"), ap(2, -80, "b")],
            vec![ap(1, -65, "new")],
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].ssid, "new");
        assert_eq!(merged[0].rssi_dbm, -63); // mean of -60 and -65 rounds away from zero
        assert_eq!(merged[1].bssid, MacAddress([0, 0, 0, 0, 0, 2]));
    }

    #[test]
    fn revealed_hidden_name_survives_later_beacon_only_sighting() {
        let merged = merge_probes([vec![ap(1, -60, "Revealed")], vec![ap(1, -60, "")]]);
        assert_eq!(merged[0].ssid, "Revealed");
        assert!(
            merged[0].hidden,
            "an AP that ever advertised no name stays flagged hidden"
        );
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(merge_probes(Vec::<Vec<AccessPoint>>::new()).is_empty());
    }
}
