//! Active-scan maps: throughput and UDP jitter measured by `openspot-speedtest` (H-11..H-13).
//!
//! A point without a usable result for the chosen test contributes **no sample at all**
//! instead of a value of zero: an unmeasured spot is unknown, not slow, and a fake zero would
//! drag the interpolation down around it. The map interpolates between the points that were
//! actually measured.

use openspot_model::{SpeedResult, Snapshot};

use crate::{Gradient, Sample};

/// Which active-scan figure a map shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpeedKind {
    HttpDown,
    HttpUp,
    TcpDown,
    TcpUp,
    UdpDown,
    UdpUp,
    UdpJitter,
}

impl SpeedKind {
    pub const ALL: [Self; 7] =
        [Self::HttpDown, Self::HttpUp, Self::TcpDown, Self::TcpUp, Self::UdpDown, Self::UdpUp, Self::UdpJitter];

    pub fn label(self) -> &'static str {
        match self {
            Self::HttpDown => "HTTP download",
            Self::HttpUp => "HTTP upload",
            Self::TcpDown => "iperf TCP download",
            Self::TcpUp => "iperf TCP upload",
            Self::UdpDown => "iperf UDP download",
            Self::UdpUp => "iperf UDP upload",
            Self::UdpJitter => "UDP jitter",
        }
    }

    pub fn unit(self) -> &'static str {
        if self == Self::UdpJitter { "ms" } else { "Mbps" }
    }

    /// Jitter is the only figure where a smaller number is better.
    pub fn lower_is_better(self) -> bool {
        self == Self::UdpJitter
    }

    /// This test's figure in `r`, if it was measured.
    pub fn value(self, r: &SpeedResult) -> Option<f64> {
        match self {
            Self::HttpDown => r.http_down_mbps,
            Self::HttpUp => r.http_up_mbps,
            Self::TcpDown => r.iperf_tcp_down_mbps,
            Self::TcpUp => r.iperf_tcp_up_mbps,
            Self::UdpDown => r.iperf_udp_down_mbps,
            Self::UdpUp => r.iperf_udp_up_mbps,
            Self::UdpJitter => r.udp_jitter_ms,
        }
    }

    /// Color ramp. Throughput stops follow common streaming guidance (about 5 Mbps for HD,
    /// 25 Mbps for 4K) and saturate at 300 Mbps, a typical good Wi-Fi 5 result. Jitter stops
    /// are reversed (green = low) around the ~30 ms ceiling usually quoted for voice.
    pub fn gradient(self) -> Gradient {
        if self.lower_is_better() {
            Gradient::new(vec![
                (0.0, [26, 150, 65]),
                (5.0, [166, 217, 106]),
                (15.0, [255, 221, 80]),
                (30.0, [253, 141, 60]),
                (60.0, [215, 25, 28]),
            ])
        } else {
            Gradient::new(vec![
                (0.0, [215, 25, 28]),
                (5.0, [253, 141, 60]),
                (25.0, [255, 221, 80]),
                (100.0, [166, 217, 106]),
                (300.0, [26, 150, 65]),
            ])
        }
    }
}

/// One sample per point that has a usable value for `kind`; `ssids` limits which networks'
/// results count (empty = all). If several networks were tested at a point the best value
/// wins (highest throughput, lowest jitter), mirroring how the signal map takes the
/// strongest network.
pub fn speed_samples(snapshot: &Snapshot, kind: SpeedKind, ssids: &[String]) -> Vec<Sample> {
    snapshot
        .points
        .iter()
        .filter_map(|p| {
            let values = p
                .speed
                .iter()
                .filter(|r| ssids.is_empty() || ssids.contains(&r.ssid))
                .filter_map(|r| kind.value(r))
                // Corrupt or hand-edited data must not poison the interpolation.
                .filter(|v| v.is_finite() && *v >= 0.0);
            let best = if kind.lower_is_better() { values.reduce(f64::min) } else { values.reduce(f64::max) };
            best.map(|value| Sample { x: p.x, y: p.y, value })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use openspot_model::ScanPoint;

    use super::*;

    fn point(x: f64, speed: Vec<SpeedResult>) -> ScanPoint {
        let mut p = ScanPoint::new(x, 0.0, 0, vec![]);
        p.speed = speed;
        p
    }

    fn r(ssid: &str, tcp_down: Option<f64>, jitter: Option<f64>) -> SpeedResult {
        SpeedResult { iperf_tcp_down_mbps: tcp_down, udp_jitter_ms: jitter, ..SpeedResult::new(ssid) }
    }

    fn snapshot() -> Snapshot {
        let mut s = Snapshot::new("s");
        s.points = vec![
            point(0.0, vec![r("a", Some(100.0), Some(4.0)), r("b", Some(250.0), Some(1.5))]),
            point(1.0, vec![]),                                   // never actively scanned
            point(2.0, vec![r("a", None, None)]),                 // test failed
            point(3.0, vec![r("a", Some(0.0), Some(0.0))]),       // a real, measured zero
            point(4.0, vec![r("a", Some(f64::NAN), Some(-3.0))]), // corrupt
        ];
        s
    }

    #[test]
    fn unmeasured_points_are_skipped_not_zeroed() {
        let v = speed_samples(&snapshot(), SpeedKind::TcpDown, &[]);
        let xs: Vec<f64> = v.iter().map(|s| s.x).collect();
        assert_eq!(xs, [0.0, 3.0], "points 1, 2 and the corrupt point 4 contribute nothing");
        assert_eq!(v[1].value, 0.0, "a measured zero stays a zero");
    }

    #[test]
    fn best_network_wins_and_jitter_prefers_lower() {
        let s = snapshot();
        assert_eq!(speed_samples(&s, SpeedKind::TcpDown, &[])[0].value, 250.0);
        assert_eq!(speed_samples(&s, SpeedKind::UdpJitter, &[])[0].value, 1.5);
    }

    #[test]
    fn ssid_filter_restricts_networks() {
        let s = snapshot();
        let only_a = speed_samples(&s, SpeedKind::TcpDown, &["a".to_string()]);
        assert_eq!(only_a[0].value, 100.0);
        assert_eq!(speed_samples(&s, SpeedKind::UdpJitter, &["a".to_string()])[0].value, 4.0);
        assert!(speed_samples(&s, SpeedKind::TcpDown, &["nope".to_string()]).is_empty());
    }

    #[test]
    fn each_kind_reads_its_own_field() {
        let full = SpeedResult {
            http_down_mbps: Some(1.0),
            http_up_mbps: Some(2.0),
            iperf_tcp_down_mbps: Some(3.0),
            iperf_tcp_up_mbps: Some(4.0),
            iperf_udp_down_mbps: Some(5.0),
            iperf_udp_up_mbps: Some(6.0),
            udp_jitter_ms: Some(7.0),
            ..SpeedResult::new("n")
        };
        let got: Vec<f64> = SpeedKind::ALL.iter().map(|k| k.value(&full).unwrap()).collect();
        assert_eq!(got, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn gradients_point_the_right_way() {
        let thr = SpeedKind::TcpDown.gradient();
        let jit = SpeedKind::UdpJitter.gradient();
        let (red, green) = ([215, 25, 28], [26, 150, 65]);
        assert_eq!((thr.color(0.0), thr.color(1000.0)), (red, green), "fast is green");
        assert_eq!((jit.color(0.0), jit.color(1000.0)), (green, red), "low jitter is green");
        assert_eq!(SpeedKind::UdpJitter.unit(), "ms");
        assert_eq!(SpeedKind::HttpUp.unit(), "Mbps");
    }

    #[test]
    fn map_with_no_measurements_is_empty_rather_than_all_zero() {
        use crate::{GridSpec, Metric, Smoothing, rasterize};
        let mut s = Snapshot::new("s");
        s.points = vec![point(0.0, vec![]), point(10.0, vec![r("a", None, None)])];
        let spec = GridSpec { width_px: 20.0, height_px: 20.0, cell_px: 2.0, extend_px: 5.0 };
        let m = Metric::Speed { kind: SpeedKind::TcpDown, ssids: vec![] };
        assert!(rasterize(&s, &m, &spec, Smoothing::default()).is_none());
    }
}
