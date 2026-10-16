//! Estimating where an access point sits from where it was heard loudest.

use openspot_model::Snapshot;
use openspot_wlan::MacAddress;

/// Strongest readings used for the estimate. Few enough to ignore far-away noisy points,
/// many enough that one bad reading cannot drag the estimate onto its own location.
const TOP_POINTS: usize = 5;
/// Fewer readings than this cannot bracket a position.
const MIN_POINTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApEstimate {
    /// Map pixel position.
    pub x: f64,
    pub y: f64,
    pub points_used: usize,
    /// Strongest RSSI seen; a weak maximum means the AP is probably far from every point.
    pub max_rssi_dbm: i32,
}

/// Power-weighted centroid of the strongest measurement points for `bssid`. Weights are
/// linear power (mW), so the nearest readings dominate, as the physics says they should.
/// `None` when the AP was heard at fewer than three points.
pub fn estimate_ap_position(snapshot: &Snapshot, bssid: &MacAddress) -> Option<ApEstimate> {
    let mut heard: Vec<(f64, f64, i32)> =
        snapshot.points.iter().filter_map(|p| p.rssi_of(bssid).map(|r| (p.x, p.y, r))).collect();
    if heard.len() < MIN_POINTS {
        return None;
    }
    heard.sort_by(|a, b| b.2.cmp(&a.2));
    heard.truncate(TOP_POINTS);
    // Relative to the strongest so the weights stay well-scaled.
    let best = heard[0].2;
    let weight = |rssi: i32| 10f64.powf(f64::from(rssi - best) / 10.0);
    let total: f64 = heard.iter().map(|h| weight(h.2)).sum();
    let x = heard.iter().map(|h| h.0 * weight(h.2)).sum::<f64>() / total;
    let y = heard.iter().map(|h| h.1 * weight(h.2)).sum::<f64>() / total;
    Some(ApEstimate { x, y, points_used: heard.len(), max_rssi_dbm: best })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::ScanPoint;
    use openspot_wlan::AccessPoint;

    const AP: MacAddress = MacAddress([0, 0, 0, 0, 0, 1]);

    /// Free-space-ish model: -30 dBm at 1 m (100 px), 3 dB per... exponent 3.
    fn rssi_at(true_pos: (f64, f64), x: f64, y: f64) -> i32 {
        let d_m = ((x - true_pos.0).hypot(y - true_pos.1) / 100.0).max(0.3);
        (-30.0 - 30.0 * d_m.log10()).round() as i32
    }

    fn grid_snapshot(true_pos: (f64, f64)) -> Snapshot {
        let mut s = Snapshot::new("s");
        for gx in 0..=8 {
            for gy in 0..=6 {
                let (x, y) = (f64::from(gx) * 100.0, f64::from(gy) * 100.0);
                let ap = AccessPoint::from_beacon("n".into(), AP, rssi_at(true_pos, x, y), 2_437_000, true, 100, vec![]);
                s.points.push(ScanPoint::new(x, y, 0, vec![ap]));
            }
        }
        s
    }

    #[test]
    fn recovers_a_known_position_closely() {
        for truth in [(250.0, 330.0), (610.0, 90.0), (400.0, 300.0)] {
            let e = estimate_ap_position(&grid_snapshot(truth), &AP).unwrap();
            let err = (e.x - truth.0).hypot(e.y - truth.1);
            assert!(err < 60.0, "truth {truth:?} estimate ({:.0},{:.0}) error {err:.0}px", e.x, e.y);
            assert_eq!(e.points_used, TOP_POINTS);
        }
    }

    #[test]
    fn needs_at_least_three_points() {
        let mut s = Snapshot::new("s");
        for i in 0..2 {
            let ap = AccessPoint::from_beacon("n".into(), AP, -50, 2_437_000, true, 100, vec![]);
            s.points.push(ScanPoint::new(f64::from(i) * 10.0, 0.0, 0, vec![ap]));
        }
        assert!(estimate_ap_position(&s, &AP).is_none());
        assert!(estimate_ap_position(&Snapshot::new("e"), &AP).is_none());
    }

    #[test]
    fn other_networks_are_ignored() {
        let s = grid_snapshot((100.0, 100.0));
        assert!(estimate_ap_position(&s, &MacAddress([9; 6])).is_none());
    }

    #[test]
    fn dominant_reading_pulls_the_estimate_toward_itself() {
        let mut s = Snapshot::new("s");
        for (x, y, r) in [(0.0, 0.0, -30), (100.0, 0.0, -70), (0.0, 100.0, -70)] {
            let ap = AccessPoint::from_beacon("n".into(), AP, r, 2_437_000, true, 100, vec![]);
            s.points.push(ScanPoint::new(x, y, 0, vec![ap]));
        }
        let e = estimate_ap_position(&s, &AP).unwrap();
        assert!(e.x < 1.0 && e.y < 1.0, "{e:?}");
        assert_eq!(e.max_rssi_dbm, -30);
    }
}
