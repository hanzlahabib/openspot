//! Per-AP coverage: one signal map per network instead of the best-of-all composite.

use openspot_model::Snapshot;
use openspot_wlan::MacAddress;

use crate::{Grid, GridSpec, Metric, Smoothing, rasterize};

/// Signal grids for each of `bssids`, in the order given. Each grid uses only that AP's
/// readings (points that did not hear it count as [`crate::FLOOR_DBM`], so its coverage
/// visibly ends). BSSIDs that were never heard anywhere in `snapshot` are omitted: their
/// map would be a uniform floor with nothing to learn from.
pub fn per_ap_grids(snapshot: &Snapshot, bssids: &[MacAddress], spec: &GridSpec, smoothing: Smoothing) -> Vec<(MacAddress, Grid)> {
    bssids
        .iter()
        .filter(|b| snapshot.points.iter().any(|p| p.rssi_of(b).is_some()))
        .filter_map(|b| Some((*b, rasterize(snapshot, &Metric::Signal(vec![*b]), spec, smoothing)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::ScanPoint;
    use openspot_wlan::AccessPoint;

    fn mac(last: u8) -> MacAddress {
        MacAddress([0, 0, 0, 0, 0, last])
    }

    fn ap(last: u8, rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), mac(last), rssi, 2_437_000, true, 100, vec![])
    }

    fn spec() -> GridSpec {
        GridSpec { width_px: 100.0, height_px: 100.0, cell_px: 10.0, extend_px: 0.0 }
    }

    /// AP 1 is strong on the left and unheard on the right; AP 2 is the opposite.
    fn two_ap_snapshot() -> Snapshot {
        let mut s = Snapshot::new("s");
        for (x, y) in [(5.0, 5.0), (5.0, 95.0)] {
            s.points.push(ScanPoint::new(x, y, 0, vec![ap(1, -40)]));
        }
        for (x, y) in [(95.0, 5.0), (95.0, 95.0)] {
            s.points.push(ScanPoint::new(x, y, 0, vec![ap(2, -45)]));
        }
        s
    }

    #[test]
    fn each_ap_gets_its_own_map_from_its_own_readings() {
        let snap = two_ap_snapshot();
        let maps = per_ap_grids(&snap, &[mac(2), mac(1)], &spec(), Smoothing::Off);
        assert_eq!(maps.iter().map(|m| m.0).collect::<Vec<_>>(), vec![mac(2), mac(1)], "order follows the request");
        let (g2, g1) = (&maps[0].1, &maps[1].1);
        // Left edge cell: AP 1 strong, AP 2 at the floor; right edge reversed.
        assert_eq!(g1.get(0, 0), Some(-40.0));
        assert_eq!(g1.get(9, 0), Some(crate::FLOOR_DBM as f32));
        assert_eq!(g2.get(9, 0), Some(-45.0));
        assert_eq!(g2.get(0, 0), Some(crate::FLOOR_DBM as f32));
        // The composite best-of-all map differs from both.
        let best = rasterize(&snap, &Metric::Signal(vec![]), &spec(), Smoothing::Off).unwrap();
        assert_ne!(&best, g1);
        assert_ne!(&best, g2);
    }

    #[test]
    fn never_heard_networks_are_omitted() {
        let maps = per_ap_grids(&two_ap_snapshot(), &[mac(9), mac(1)], &spec(), Smoothing::Off);
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].0, mac(1));
        assert!(per_ap_grids(&Snapshot::new("empty"), &[mac(1)], &spec(), Smoothing::Off).is_empty());
    }
}
