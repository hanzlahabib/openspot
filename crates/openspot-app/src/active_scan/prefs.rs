//! Active-scan preferences (A-05) and their translation into a speedtest suite.

use std::time::Duration;

use openspot_speedtest::http::HttpConfig;
use openspot_speedtest::iperf::{DEFAULT_PORT, UdpBitrate};
use openspot_speedtest::suite::{HttpSuite, IperfSuite, SuiteConfig};
use serde::{Deserialize, Serialize};

/// iperf3 refuses more than 128 parallel streams (`MAX_STREAMS` in its source).
const MAX_STREAMS: u8 = 128;
/// Longest test offered: a survey point someone stands on for ten minutes is already absurd.
const MAX_DURATION_SECS: u32 = 600;
const MAX_HTTP_MB: u32 = 1024;
const MIN_HTTP_TIMEOUT_SECS: u64 = 5;
const MAX_HTTP_TIMEOUT_SECS: u64 = 600;
/// Longest wait after connecting: DHCP and captive checks settle within seconds.
const MAX_DELAY_SECS: u32 = 120;
/// 64 MiB of socket buffer is far past any useful TCP window.
const MAX_WINDOW_KIB: u32 = 64 * 1024;
const BYTES_PER_MB: u64 = 1_000_000;
const BYTES_PER_KIB: u32 = 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActiveScanPrefs {
    /// Survey toolbar toggle: run the tests after each passive scan.
    pub enabled: bool,
    pub use_http: bool,
    pub use_iperf_tcp: bool,
    pub use_iperf_udp: bool,
    /// Empty by default on purpose: OpenSpot never contacts a third-party server unless the
    /// user names one.
    pub http_download_url: String,
    pub http_upload_url: String,
    pub http_download_mb: u32,
    pub http_upload_mb: u32,
    pub http_timeout_secs: u64,
    pub iperf_host: String,
    pub iperf_port: u16,
    /// Path of the `iperf3` executable; empty = look it up on `PATH`.
    pub iperf_binary: String,
    pub duration_secs: u32,
    pub streams: u8,
    /// TCP window / socket buffer in KiB; 0 keeps the OS default.
    pub window_kib: u32,
    /// UDP sending rate in Mbit/s; 0 = unlimited.
    pub udp_bitrate_mbps: f64,
    /// Send UDP at the TCP result of the same direction when TCP ran.
    pub udp_reuse_tcp: bool,
    /// Pause after connecting to a network before measuring (DHCP, route settling).
    pub delay_secs: u32,
    /// Networks to test, by their saved Windows profile name (usually the SSID). Empty = test
    /// whatever the adapter is connected to, without switching.
    pub ssids: Vec<String>,
}

impl Default for ActiveScanPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            use_http: false,
            use_iperf_tcp: true,
            use_iperf_udp: false,
            http_download_url: String::new(),
            http_upload_url: String::new(),
            http_download_mb: 25,
            http_upload_mb: 10,
            http_timeout_secs: 60,
            iperf_host: String::new(),
            iperf_port: DEFAULT_PORT,
            iperf_binary: String::new(),
            duration_secs: 10,
            streams: 1,
            window_kib: 0,
            udp_bitrate_mbps: 50.0,
            udp_reuse_tcp: true,
            delay_secs: 2,
            ssids: Vec::new(),
        }
    }
}

impl ActiveScanPrefs {
    /// Clamps every number into its supported range and drops blank SSIDs. Applied to
    /// preferences read from disk, which may have been edited by hand.
    pub fn sanitized(mut self) -> Self {
        self.http_download_mb = self.http_download_mb.clamp(1, MAX_HTTP_MB);
        self.http_upload_mb = self.http_upload_mb.clamp(1, MAX_HTTP_MB);
        self.http_timeout_secs = self.http_timeout_secs.clamp(MIN_HTTP_TIMEOUT_SECS, MAX_HTTP_TIMEOUT_SECS);
        if self.iperf_port == 0 {
            self.iperf_port = DEFAULT_PORT;
        }
        self.duration_secs = self.duration_secs.clamp(1, MAX_DURATION_SECS);
        self.streams = self.streams.clamp(1, MAX_STREAMS);
        self.window_kib = self.window_kib.min(MAX_WINDOW_KIB);
        if !self.udp_bitrate_mbps.is_finite() || self.udp_bitrate_mbps < 0.0 {
            self.udp_bitrate_mbps = 0.0;
        }
        self.delay_secs = self.delay_secs.min(MAX_DELAY_SECS);
        self.ssids = self.ssids.iter().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
        self
    }

    pub fn http_config(&self, url: &str, megabytes: u32) -> HttpConfig {
        HttpConfig {
            timeout: Duration::from_secs(self.http_timeout_secs),
            ..HttpConfig::new(url, u64::from(megabytes) * BYTES_PER_MB)
        }
    }

    fn udp_bitrate(&self) -> UdpBitrate {
        if self.udp_bitrate_mbps > 0.0 { UdpBitrate::Mbps(self.udp_bitrate_mbps) } else { UdpBitrate::Unlimited }
    }

    /// The suite to run, or a message telling the user what to fill in first.
    pub fn to_suite(&self) -> Result<SuiteConfig, String> {
        let p = self.clone().sanitized();
        if !(p.use_http || p.use_iperf_tcp || p.use_iperf_udp) {
            return Err("Active scan: choose at least one test in the settings".into());
        }
        let http = if p.use_http {
            for (what, url) in [("download", &p.http_download_url), ("upload", &p.http_upload_url)] {
                let url = url.trim();
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Err(format!("Active scan: set an http(s) {what} URL in the settings"));
                }
            }
            Some(HttpSuite {
                download_url: p.http_download_url.trim().to_owned(),
                upload_url: p.http_upload_url.trim().to_owned(),
                download_bytes: u64::from(p.http_download_mb) * BYTES_PER_MB,
                upload_bytes: u64::from(p.http_upload_mb) * BYTES_PER_MB,
                timeout: Duration::from_secs(p.http_timeout_secs),
            })
        } else {
            None
        };
        let iperf = if p.use_iperf_tcp || p.use_iperf_udp {
            if p.iperf_host.trim().is_empty() {
                return Err("Active scan: set the iperf3 server address in the settings".into());
            }
            Some(IperfSuite {
                host: p.iperf_host.trim().to_owned(),
                port: p.iperf_port,
                tcp: p.use_iperf_tcp,
                udp: p.use_iperf_udp,
                duration_secs: p.duration_secs,
                streams: p.streams,
                window_bytes: (p.window_kib > 0).then(|| p.window_kib * BYTES_PER_KIB),
                udp_bitrate: p.udp_bitrate(),
                udp_reuse_tcp: p.udp_reuse_tcp,
                binary: (!p.iperf_binary.trim().is_empty()).then(|| p.iperf_binary.trim().into()),
            })
        } else {
            None
        };
        Ok(SuiteConfig { http, iperf })
    }
}

#[cfg(test)]
mod tests {
    use openspot_speedtest::suite::Stage;

    use super::*;

    fn ready() -> ActiveScanPrefs {
        ActiveScanPrefs {
            use_http: true,
            use_iperf_tcp: true,
            use_iperf_udp: true,
            http_download_url: " http://srv/down ".into(),
            http_upload_url: "https://srv/up".into(),
            iperf_host: " 10.0.0.5 ".into(),
            ..ActiveScanPrefs::default()
        }
    }

    #[test]
    fn full_selection_builds_every_stage_with_converted_units() {
        let mut p = ready();
        p.window_kib = 256;
        p.udp_bitrate_mbps = 25.0;
        let s = p.to_suite().unwrap();
        assert_eq!(s.stages().len(), 6);
        assert_eq!(s.stages()[0], Stage::HttpDown);
        let h = s.http.unwrap();
        assert_eq!((h.download_bytes, h.upload_bytes), (25_000_000, 10_000_000));
        assert_eq!(h.download_url, "http://srv/down", "URL is trimmed");
        let i = s.iperf.unwrap();
        assert_eq!((i.host.as_str(), i.port, i.duration_secs, i.streams), ("10.0.0.5", 5201, 10, 1));
        assert_eq!(i.window_bytes, Some(262_144));
        assert_eq!(i.udp_bitrate, UdpBitrate::Mbps(25.0));
        assert!(i.binary.is_none());
    }

    #[test]
    fn zero_window_and_zero_bitrate_mean_defaults() {
        let mut p = ready();
        p.window_kib = 0;
        p.udp_bitrate_mbps = 0.0;
        let i = p.to_suite().unwrap().iperf.unwrap();
        assert_eq!((i.window_bytes, i.udp_bitrate), (None, UdpBitrate::Unlimited));
    }

    #[test]
    fn missing_settings_produce_actionable_errors() {
        assert!(ActiveScanPrefs { use_iperf_tcp: false, ..ActiveScanPrefs::default() }.to_suite().unwrap_err().contains("at least one"));
        assert!(ActiveScanPrefs::default().to_suite().unwrap_err().contains("iperf3 server"));
        let mut p = ready();
        p.http_upload_url = "ftp://x".into();
        assert!(p.to_suite().unwrap_err().contains("upload URL"));
        p.http_upload_url = "https://ok".into();
        p.http_download_url = String::new();
        assert!(p.to_suite().unwrap_err().contains("download URL"));
    }

    #[test]
    fn http_only_does_not_require_an_iperf_host() {
        let p = ActiveScanPrefs { use_iperf_tcp: false, ..ready() };
        let p = ActiveScanPrefs { use_iperf_udp: false, ..p };
        let s = p.to_suite().unwrap();
        assert!(s.iperf.is_none() && s.http.is_some());
    }

    #[test]
    fn sanitize_clamps_hand_edited_values() {
        let p = ActiveScanPrefs {
            http_download_mb: 0,
            http_upload_mb: 99_999,
            http_timeout_secs: 0,
            iperf_port: 0,
            duration_secs: 0,
            streams: 0,
            window_kib: u32::MAX,
            udp_bitrate_mbps: f64::NAN,
            delay_secs: 9_999,
            ssids: vec![" Home ".into(), "  ".into(), "Lab".into()],
            ..ActiveScanPrefs::default()
        }
        .sanitized();
        assert_eq!((p.http_download_mb, p.http_upload_mb, p.http_timeout_secs), (1, MAX_HTTP_MB, MIN_HTTP_TIMEOUT_SECS));
        assert_eq!((p.iperf_port, p.duration_secs, p.streams), (DEFAULT_PORT, 1, 1));
        assert_eq!((p.window_kib, p.udp_bitrate_mbps, p.delay_secs), (MAX_WINDOW_KIB, 0.0, MAX_DELAY_SECS));
        assert_eq!(p.ssids, ["Home", "Lab"]);
        assert_eq!(ActiveScanPrefs { streams: 200, ..ActiveScanPrefs::default() }.sanitized().streams, MAX_STREAMS);
    }

    #[test]
    fn json_round_trip_and_old_files_get_defaults() {
        let p = ready();
        let back: ActiveScanPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let partial: ActiveScanPrefs = serde_json::from_str(r#"{"iperf_host":"h"}"#).unwrap();
        assert_eq!(partial.iperf_host, "h");
        assert_eq!(partial.duration_secs, ActiveScanPrefs::default().duration_secs);
    }
}
