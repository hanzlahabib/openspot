//! Wireless transmit-rate map (H-10): the negotiated rate of the adapter's own link at each
//! measurement point.
//!
//! The figure belongs to the network the laptop was connected to while measuring, not to every
//! access point it could hear, so it only says something about the AP the laptop used at each
//! spot. Like the speed maps, a point without a usable rate (not connected, or the driver
//! reported none) contributes no sample instead of a zero.

use openspot_model::Snapshot;

use crate::{Gradient, Sample};

/// Unit shown on the legend.
pub const TX_RATE_UNIT: &str = "Mbps";

/// Color ramp over the negotiated rate. 6-54 Mbps is a legacy or very weak link (red to
/// orange), 150-300 Mbps a typical 20/40 MHz Wi-Fi 4 / weak Wi-Fi 5 link (yellow), 600 Mbps
/// a good 2x2 Wi-Fi 5/6 link, and 1200 Mbps is the 2x2 80 MHz Wi-Fi 6 maximum where the ramp
/// saturates. A user range (H-15) can rescale it for other hardware.
pub fn gradient() -> Gradient {
    Gradient::new(vec![
        (0.0, [215, 25, 28]),
        (50.0, [253, 141, 60]),
        (200.0, [255, 221, 80]),
        (600.0, [166, 217, 106]),
        (1200.0, [26, 150, 65]),
    ])
}

/// One sample per point whose link reported a transmit rate above zero.
pub fn tx_rate_samples(snapshot: &Snapshot) -> Vec<Sample> {
    snapshot
        .points
        .iter()
        .filter_map(|p| {
            let rate = p.link.as_ref()?.tx_rate_mbps;
            // Zero means "not reported"; non-finite values can only come from a corrupt file.
            (rate.is_finite() && rate > 0.0).then_some(Sample { x: p.x, y: p.y, value: rate })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use openspot_model::{LinkInfo, ScanPoint};
    use openspot_wlan::MacAddress;

    use super::*;

    fn link(tx: f64) -> LinkInfo {
        LinkInfo { bssid: MacAddress([2, 0, 0, 0, 0, 1]), tx_rate_mbps: tx, rx_rate_mbps: 1.0 }
    }

    fn point(x: f64, link: Option<LinkInfo>) -> ScanPoint {
        let mut p = ScanPoint::new(x, 0.0, 0, vec![]);
        p.link = link;
        p
    }

    #[test]
    fn only_points_with_a_positive_rate_yield_samples() {
        let mut s = Snapshot::new("s");
        s.points.push(point(0.0, Some(link(866.7))));
        s.points.push(point(1.0, None)); // not connected
        s.points.push(point(2.0, Some(link(0.0)))); // connected, no rate reported
        s.points.push(point(3.0, Some(link(f64::NAN))));
        s.points.push(point(4.0, Some(link(54.0))));
        let v = tx_rate_samples(&s);
        assert_eq!(v.iter().map(|x| (x.x, x.value)).collect::<Vec<_>>(), [(0.0, 866.7), (4.0, 54.0)]);
    }

    #[test]
    fn uses_the_transmit_rate_not_the_receive_rate() {
        let mut s = Snapshot::new("s");
        s.points.push(point(0.0, Some(LinkInfo { tx_rate_mbps: 100.0, rx_rate_mbps: 400.0, ..link(0.0) })));
        assert_eq!(tx_rate_samples(&s)[0].value, 100.0);
    }

    #[test]
    fn gradient_runs_from_red_to_green_and_saturates() {
        let g = gradient();
        assert_eq!(g.range(), (0.0, 1200.0));
        assert_eq!(g.color(0.0), [215, 25, 28]);
        assert_eq!(g.color(5000.0), [26, 150, 65]);
        assert_ne!(g.color(100.0), g.color(900.0));
    }
}
