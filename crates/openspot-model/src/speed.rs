//! Active-scan (throughput) results measured at one scan point.

use openspot_wlan::MacAddress;

/// Throughput measured while associated to one network at a scan point.
///
/// Every measurement is `Option`: `None` means "not measured or failed", never zero. A test
/// that could not run records its reason in [`SpeedResult::errors`] instead, so heatmaps skip
/// the point rather than painting a dead spot (see `openspot_heatmap::speed_samples`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpeedResult {
    pub ssid: String,
    /// The BSSID the adapter was associated to during the test, when known.
    pub bssid: Option<MacAddress>,
    pub http_down_mbps: Option<f64>,
    pub http_up_mbps: Option<f64>,
    pub iperf_tcp_down_mbps: Option<f64>,
    pub iperf_tcp_up_mbps: Option<f64>,
    pub iperf_udp_down_mbps: Option<f64>,
    pub iperf_udp_up_mbps: Option<f64>,
    pub udp_jitter_ms: Option<f64>,
    pub udp_loss_pct: Option<f64>,
    /// Human-readable failures, one per test that did not produce a number.
    pub errors: Vec<String>,
}

impl SpeedResult {
    pub fn new(ssid: impl Into<String>) -> Self {
        Self { ssid: ssid.into(), ..Self::default() }
    }

    /// True when no test produced a number (only errors, or nothing at all).
    pub fn is_empty(&self) -> bool {
        [
            self.http_down_mbps,
            self.http_up_mbps,
            self.iperf_tcp_down_mbps,
            self.iperf_tcp_up_mbps,
            self.iperf_udp_down_mbps,
            self.iperf_udp_up_mbps,
            self.udp_jitter_ms,
            self.udp_loss_pct,
        ]
        .iter()
        .all(Option::is_none)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_until_a_number_is_present() {
        let mut r = SpeedResult::new("net");
        r.errors.push("no route".into());
        assert!(r.is_empty());
        r.udp_loss_pct = Some(0.0);
        assert!(!r.is_empty(), "a measured zero is a result");
    }
}
