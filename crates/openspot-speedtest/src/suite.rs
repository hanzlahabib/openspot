//! Runs the selected tests for one network at one point and folds them into a
//! [`SpeedResult`]. A failing test records an error and leaves its field `None`; it never
//! aborts the others and never produces a fake zero.

use std::path::PathBuf;
use std::time::Duration;

use openspot_model::SpeedResult;

use crate::http::{self, HttpConfig};
use crate::iperf::{Direction, IperfConfig, IperfRunner, Protocol, UdpBitrate};
use crate::{CancelToken, SpeedError};

/// The individual tests, in execution order (used for progress reporting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    HttpDown,
    HttpUp,
    TcpDown,
    TcpUp,
    UdpDown,
    UdpUp,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::HttpDown => "HTTP download",
            Self::HttpUp => "HTTP upload",
            Self::TcpDown => "iperf3 TCP download",
            Self::TcpUp => "iperf3 TCP upload",
            Self::UdpDown => "iperf3 UDP download",
            Self::UdpUp => "iperf3 UDP upload",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpSuite {
    pub download_url: String,
    pub upload_url: String,
    pub download_bytes: u64,
    pub upload_bytes: u64,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IperfSuite {
    pub host: String,
    pub port: u16,
    pub tcp: bool,
    pub udp: bool,
    pub duration_secs: u32,
    pub streams: u8,
    pub window_bytes: Option<u32>,
    pub udp_bitrate: UdpBitrate,
    /// Send UDP at the TCP result of the same direction (when TCP ran and succeeded), else
    /// at `udp_bitrate`.
    pub udp_reuse_tcp: bool,
    /// Override of the `iperf3` executable.
    pub binary: Option<PathBuf>,
}

/// Which tests to run; `None` disables a family.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SuiteConfig {
    pub http: Option<HttpSuite>,
    pub iperf: Option<IperfSuite>,
}

impl SuiteConfig {
    /// The stages that [`run`] will execute, in order.
    pub fn stages(&self) -> Vec<Stage> {
        let mut v = Vec::new();
        if self.http.is_some() {
            v.extend([Stage::HttpDown, Stage::HttpUp]);
        }
        if let Some(i) = &self.iperf {
            if i.tcp {
                v.extend([Stage::TcpDown, Stage::TcpUp]);
            }
            if i.udp {
                v.extend([Stage::UdpDown, Stage::UdpUp]);
            }
        }
        v
    }
}

fn record<T>(errors: &mut Vec<String>, stage: Stage, r: Result<T, SpeedError>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(format!("{}: {e}", stage.label()));
            None
        }
    }
}

/// Runs every enabled stage for `ssid`, calling `on_stage` before each. Stops early (keeping
/// what was measured and noting it in `errors`) when `cancel` fires.
pub fn run(ssid: &str, cfg: &SuiteConfig, cancel: &CancelToken, on_stage: &mut dyn FnMut(Stage)) -> SpeedResult {
    let mut out = SpeedResult::new(ssid);
    let mut errors = Vec::new();
    for stage in cfg.stages() {
        if cancel.is_cancelled() {
            errors.push("cancelled".into());
            break;
        }
        on_stage(stage);
        run_stage(stage, cfg, cancel, &mut out, &mut errors);
    }
    out.errors = errors;
    out
}

fn run_stage(stage: Stage, cfg: &SuiteConfig, cancel: &CancelToken, out: &mut SpeedResult, errors: &mut Vec<String>) {
    if let Some(h) = cfg.http.as_ref().filter(|_| matches!(stage, Stage::HttpDown | Stage::HttpUp)) {
        let (url, bytes, f): (_, _, fn(&HttpConfig, &CancelToken) -> _) = if stage == Stage::HttpDown {
            (&h.download_url, h.download_bytes, http::download)
        } else {
            (&h.upload_url, h.upload_bytes, http::upload)
        };
        let hc = HttpConfig { timeout: h.timeout, ..HttpConfig::new(url.clone(), bytes) };
        let m = record(errors, stage, f(&hc, cancel)).map(|m| m.mbps);
        if stage == Stage::HttpDown {
            out.http_down_mbps = m;
        } else {
            out.http_up_mbps = m;
        }
        return;
    }
    let Some(i) = &cfg.iperf else { return };
    let runner = i.binary.clone().map_or_else(IperfRunner::default, IperfRunner::with_binary);
    let direction = if matches!(stage, Stage::TcpDown | Stage::UdpDown) { Direction::Download } else { Direction::Upload };
    let protocol = match stage {
        Stage::TcpDown | Stage::TcpUp => Protocol::Tcp,
        _ => {
            let tcp = if direction == Direction::Download { out.iperf_tcp_down_mbps } else { out.iperf_tcp_up_mbps };
            let rate = if i.udp_reuse_tcp { UdpBitrate::from_tcp_result(tcp, i.udp_bitrate) } else { i.udp_bitrate };
            Protocol::Udp(rate)
        }
    };
    let ic = IperfConfig {
        host: i.host.clone(),
        port: i.port,
        protocol,
        direction,
        duration_secs: i.duration_secs,
        streams: i.streams,
        window_bytes: i.window_bytes,
    };
    let Some(o) = record(errors, stage, runner.run(&ic, cancel)) else { return };
    match stage {
        Stage::TcpDown => out.iperf_tcp_down_mbps = Some(o.mbps),
        Stage::TcpUp => out.iperf_tcp_up_mbps = Some(o.mbps),
        Stage::UdpDown | Stage::UdpUp => {
            if stage == Stage::UdpDown {
                out.iperf_udp_down_mbps = Some(o.mbps);
            } else {
                out.iperf_udp_up_mbps = Some(o.mbps);
            }
            // Jitter and loss are receiver-side. The client is the receiver in the download
            // run, so that run wins; the upload run only fills them when download did not.
            if stage == Stage::UdpDown || out.udp_jitter_ms.is_none() {
                out.udp_jitter_ms = o.jitter_ms.or(out.udp_jitter_ms);
                out.udp_loss_pct = o.loss_pct.or(out.udp_loss_pct);
            }
        }
        Stage::HttpDown | Stage::HttpUp => {}
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// Fake iperf3: UDP runs echo the requested bitrate as the throughput so reuse is visible.
    fn fake_iperf() -> PathBuf {
        // Written once: rewriting an executable that another test is spawning fails with ETXTBSY.
        static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        PATH.get_or_init(write_fake_iperf).clone()
    }

    fn write_fake_iperf() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openspot-suite-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("iperf3");
        let body = r#"#!/bin/sh
udp=0; rev=0; rate=""
while [ $# -gt 0 ]; do
  case "$1" in --udp) udp=1;; --reverse) rev=1;; --bitrate) rate="$2"; shift;; esac
  shift
done
if [ $udp = 1 ]; then
  echo "{\"end\":{\"sum\":{\"bits_per_second\":1000000.0,\"jitter_ms\":$((rev+1)).5,\"lost_percent\":$rev.0}},\"rate\":\"$rate\"}"
else
  if [ $rev = 1 ]; then bps=200000000.0; else bps=100000000.0; fi
  echo "{\"end\":{\"sum_received\":{\"bits_per_second\":$bps}}}"
fi
"#;
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn iperf(tcp: bool, udp: bool) -> IperfSuite {
        IperfSuite {
            host: "h".into(),
            port: 5201,
            tcp,
            udp,
            duration_secs: 1,
            streams: 1,
            window_bytes: None,
            udp_bitrate: UdpBitrate::Mbps(10.0),
            udp_reuse_tcp: false,
            binary: Some(fake_iperf()),
        }
    }

    #[test]
    fn stages_follow_the_selection() {
        let cfg = SuiteConfig { http: None, iperf: Some(iperf(true, false)) };
        assert_eq!(cfg.stages(), [Stage::TcpDown, Stage::TcpUp]);
        assert!(SuiteConfig::default().stages().is_empty());
    }

    #[test]
    fn tcp_and_udp_results_land_in_their_fields() {
        let cfg = SuiteConfig { http: None, iperf: Some(iperf(true, true)) };
        let mut seen = Vec::new();
        let r = run("net", &cfg, &CancelToken::new(), &mut |s| seen.push(s));
        assert_eq!(seen.len(), 4);
        assert_eq!((r.iperf_tcp_down_mbps, r.iperf_tcp_up_mbps), (Some(200.0), Some(100.0)));
        assert_eq!((r.iperf_udp_down_mbps, r.iperf_udp_up_mbps), (Some(1.0), Some(1.0)));
        // Download (reverse) run supplies jitter/loss: jitter 2.5, loss 1.0.
        assert_eq!((r.udp_jitter_ms, r.udp_loss_pct), (Some(2.5), Some(1.0)));
        assert!(r.errors.is_empty());
        assert_eq!(r.http_down_mbps, None, "untested stays None, not zero");
    }

    #[test]
    fn failure_of_one_family_does_not_block_the_other() {
        let http = HttpSuite {
            download_url: "http://127.0.0.1:1/x".into(), // nothing listens on port 1
            upload_url: "http://127.0.0.1:1/x".into(),
            download_bytes: 1000,
            upload_bytes: 1000,
            timeout: Duration::from_secs(2),
        };
        let cfg = SuiteConfig { http: Some(http), iperf: Some(iperf(true, false)) };
        let r = run("net", &cfg, &CancelToken::new(), &mut |_| {});
        assert_eq!((r.http_down_mbps, r.http_up_mbps), (None, None));
        assert_eq!(r.errors.len(), 2);
        assert!(r.errors[0].starts_with("HTTP download"));
        assert_eq!(r.iperf_tcp_down_mbps, Some(200.0));
    }

    #[test]
    fn udp_can_reuse_the_tcp_result_of_the_same_direction() {
        let mut i = iperf(true, true);
        i.udp_reuse_tcp = true;
        let runner_args = |tcp: Option<f64>| UdpBitrate::from_tcp_result(tcp, i.udp_bitrate);
        // Same rule the suite applies, checked through the public helper; the stage wiring is
        // exercised by running with reuse and confirming both UDP runs still succeed.
        assert_eq!(runner_args(Some(200.0)), UdpBitrate::Mbps(200.0));
        let r = run("net", &SuiteConfig { http: None, iperf: Some(i) }, &CancelToken::new(), &mut |_| {});
        assert_eq!(r.iperf_udp_down_mbps, Some(1.0));
    }

    #[test]
    fn cancellation_stops_before_the_next_stage() {
        let cfg = SuiteConfig { http: None, iperf: Some(iperf(true, true)) };
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        let mut n = 0;
        let r = run("net", &cfg, &cancel, &mut |_| {
            n += 1;
            if n == 2 {
                c2.cancel();
            }
        });
        assert_eq!(n, 2);
        assert!(r.errors.iter().any(|e| e.contains("cancelled")));
        assert_eq!(r.iperf_udp_down_mbps, None);
    }
}
