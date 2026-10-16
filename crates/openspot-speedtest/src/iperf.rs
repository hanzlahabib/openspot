//! iperf3 runner (roadmap A-03 TCP, A-04 UDP) around the system `iperf3` binary.
//!
//! Output is parsed from `--json` per iperf3's documented schema (iperf3 man page and
//! `docs/` "JSON output"): `end.sum_sent` / `end.sum_received` for TCP, `end.sum` for UDP
//! (`bits_per_second`, `jitter_ms`, `lost_packets`, `packets`, `lost_percent`), and a top-level
//! `error` string when the run failed. Argument building and parsing are pure functions.
//!
//! UNVERIFIED: no real `iperf3` server was available when this was written. The parser is
//! tested against fixtures written from the documented schema, and the process handling
//! against shell-script stand-ins (Unix only), not against a live iperf3.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{BITS_PER_MEGABIT, CancelToken, SpeedError};

/// iperf3's default control/data port.
pub const DEFAULT_PORT: u16 = 5201;
/// Extra time on top of the test duration before the child is killed: covers connection
/// setup, the omit period, and the server's end-of-test summary exchange.
const KILL_GRACE: Duration = Duration::from_secs(15);
/// How often the supervising loop checks for exit, cancellation and the deadline.
const POLL: Duration = Duration::from_millis(50);
/// `iperf3 -b 0` removes the bitrate cap (UDP otherwise defaults to only 1 Mbit/s).
const UNLIMITED_BITRATE: &str = "0";

/// Direction from the client's (survey laptop's) point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Client receives: iperf3 `--reverse`.
    Download,
    /// Client sends.
    Upload,
}

/// UDP sending rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UdpBitrate {
    /// No cap (`-b 0`); can saturate the link and measures loss under overload.
    Unlimited,
    /// Fixed rate in Mbit/s.
    Mbps(f64),
}

impl UdpBitrate {
    /// Cap to use after a TCP run: "reuse TCP result" sends UDP at the TCP throughput
    /// (a realistic offered load) and falls back to `fallback` when TCP gave no number.
    pub fn from_tcp_result(tcp_mbps: Option<f64>, fallback: Self) -> Self {
        match tcp_mbps {
            Some(m) if m.is_finite() && m > 0.0 => Self::Mbps(m),
            _ => fallback,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Protocol {
    Tcp,
    Udp(UdpBitrate),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IperfConfig {
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub direction: Direction,
    /// Test length in seconds (`-t`).
    pub duration_secs: u32,
    /// Parallel streams (`-P`).
    pub streams: u8,
    /// Socket buffer / window in bytes (`-w`); `None` keeps the OS default.
    pub window_bytes: Option<u32>,
}

impl IperfConfig {
    pub fn new(host: impl Into<String>, protocol: Protocol, direction: Direction) -> Self {
        Self { host: host.into(), port: DEFAULT_PORT, protocol, direction, duration_secs: 10, streams: 1, window_bytes: None }
    }
}

/// Formats a Mbit/s rate for `-b`, using iperf3's `M` suffix (megabits per second, 10^6).
fn bitrate_arg(mbps: f64) -> String {
    format!("{mbps:.3}M")
}

/// The argument list for one run. Pure, so every flag combination is unit-tested.
pub fn build_args(cfg: &IperfConfig) -> Result<Vec<String>, SpeedError> {
    if cfg.host.trim().is_empty() {
        return Err(SpeedError::Config("iperf3 server host is empty".into()));
    }
    if cfg.duration_secs == 0 || cfg.streams == 0 {
        return Err(SpeedError::Config("duration and streams must be at least 1".into()));
    }
    let mut a = vec![
        "--client".into(),
        cfg.host.clone(),
        "--port".into(),
        cfg.port.to_string(),
        "--time".into(),
        cfg.duration_secs.to_string(),
        "--parallel".into(),
        cfg.streams.to_string(),
        "--json".into(),
    ];
    if cfg.direction == Direction::Download {
        a.push("--reverse".into());
    }
    if let Some(w) = cfg.window_bytes {
        a.push("--window".into());
        a.push(w.to_string());
    }
    if let Protocol::Udp(rate) = cfg.protocol {
        a.push("--udp".into());
        a.push("--bitrate".into());
        a.push(match rate {
            UdpBitrate::Unlimited => UNLIMITED_BITRATE.into(),
            UdpBitrate::Mbps(m) if m.is_finite() && m > 0.0 => bitrate_arg(m),
            UdpBitrate::Mbps(m) => return Err(SpeedError::Config(format!("invalid UDP bitrate {m}"))),
        });
    }
    Ok(a)
}

/// What one iperf3 run measured. `jitter_ms`/`loss_pct` are UDP only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IperfOutcome {
    pub mbps: f64,
    pub jitter_ms: Option<f64>,
    pub loss_pct: Option<f64>,
}

#[derive(Deserialize)]
struct Report {
    error: Option<String>,
    end: Option<End>,
}

#[derive(Deserialize)]
struct End {
    sum_sent: Option<Sum>,
    sum_received: Option<Sum>,
    sum: Option<Sum>,
}

#[derive(Deserialize, Clone, Copy)]
struct Sum {
    bits_per_second: f64,
    jitter_ms: Option<f64>,
    lost_percent: Option<f64>,
}

impl Sum {
    fn outcome(self) -> IperfOutcome {
        IperfOutcome { mbps: self.bits_per_second / BITS_PER_MEGABIT, jitter_ms: self.jitter_ms, loss_pct: self.lost_percent }
    }
}

/// Parses iperf3 `--json` output. A top-level `error` wins over any partial data.
///
/// Receiver-side figures are used: for TCP `end.sum_received` (what the far end actually got,
/// the conservative number; `sum_sent` counts bytes merely handed to the kernel), for UDP
/// `end.sum`, which carries the receiver's jitter and loss.
pub fn parse_report(json: &str, protocol: Protocol) -> Result<IperfOutcome, SpeedError> {
    let report: Report = serde_json::from_str(json).map_err(|e| SpeedError::IperfOutput(e.to_string()))?;
    if let Some(msg) = report.error {
        return Err(SpeedError::Iperf(msg));
    }
    let end = report.end.ok_or_else(|| SpeedError::IperfOutput("no \"end\" section (test did not finish)".into()))?;
    let sum = match protocol {
        Protocol::Tcp => end.sum_received.or(end.sum_sent),
        Protocol::Udp(_) => end.sum.or(end.sum_received),
    }
    .ok_or_else(|| SpeedError::IperfOutput("no throughput summary in \"end\"".into()))?;
    if !sum.bits_per_second.is_finite() || sum.bits_per_second < 0.0 {
        return Err(SpeedError::IperfOutput("invalid bits_per_second".into()));
    }
    Ok(sum.outcome())
}

/// Parses `iperf3 --version` output such as `iperf 3.12 (cJSON 1.7.15)` into `"3.12"`.
pub fn parse_version(output: &str) -> Option<String> {
    let first = output.lines().next()?;
    let rest = first.strip_prefix("iperf")?.trim_start();
    let v = rest.split_whitespace().next()?;
    v.chars().next().is_some_and(|c| c.is_ascii_digit()).then(|| v.to_owned())
}

/// Runs the `iperf3` executable.
#[derive(Debug, Clone)]
pub struct IperfRunner {
    binary: PathBuf,
}

impl Default for IperfRunner {
    fn default() -> Self {
        Self { binary: PathBuf::from("iperf3") }
    }
}

impl IperfRunner {
    /// Uses a specific executable (a path, or a name resolved through `PATH`).
    pub fn with_binary(binary: impl Into<PathBuf>) -> Self {
        Self { binary: binary.into() }
    }

    /// Version of the installed `iperf3`, e.g. `"3.12"`.
    pub fn version(&self) -> Result<String, SpeedError> {
        let out = Command::new(&self.binary)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| SpeedError::IperfSpawn(e.to_string()))?;
        // Some builds print the banner on stderr.
        let text = String::from_utf8_lossy(if out.stdout.is_empty() { &out.stderr } else { &out.stdout }).into_owned();
        parse_version(&text).ok_or_else(|| SpeedError::IperfOutput(format!("unrecognised version output: {}", text.trim())))
    }

    /// Connection test for the preferences dialog: a one-second TCP run proves the binary
    /// exists, the server is reachable and answers. Returns the measured Mbps.
    pub fn test_connection(&self, host: &str, port: u16, cancel: &CancelToken) -> Result<IperfOutcome, SpeedError> {
        let mut cfg = IperfConfig::new(host, Protocol::Tcp, Direction::Upload);
        cfg.port = port;
        cfg.duration_secs = 1;
        self.run(&cfg, cancel)
    }

    /// Runs one test. The child is killed on cancellation or when it outlives the test
    /// duration plus a grace period.
    pub fn run(&self, cfg: &IperfConfig, cancel: &CancelToken) -> Result<IperfOutcome, SpeedError> {
        let args = build_args(cfg)?;
        let deadline = Duration::from_secs(u64::from(cfg.duration_secs)) + KILL_GRACE;
        let stdout = self.supervise(&args, deadline, cancel)?;
        parse_report(&stdout, cfg.protocol)
    }

    fn supervise(&self, args: &[String], deadline: Duration, cancel: &CancelToken) -> Result<String, SpeedError> {
        let mut child = Command::new(&self.binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| SpeedError::IperfSpawn(e.to_string()))?;
        // Drain stdout on a thread so a full pipe can never stall the child.
        let mut pipe = child.stdout.take().ok_or_else(|| SpeedError::IperfSpawn("no stdout pipe".into()))?;
        let reader = thread::spawn(move || {
            let mut s = String::new();
            let _ = pipe.read_to_string(&mut s);
            s
        });
        let started = Instant::now();
        let status = loop {
            if cancel.is_cancelled() || started.elapsed() > deadline {
                // Best effort: the child may have exited between the check and the kill.
                let _ = child.kill();
                let _ = child.wait();
                // Not joined: a grandchild that inherited the pipe could keep it open and
                // block us; the detached reader ends when the pipe finally closes.
                drop(reader);
                return Err(if cancel.is_cancelled() { SpeedError::Cancelled } else { SpeedError::Timeout(deadline) });
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(POLL),
                Err(e) => return Err(SpeedError::IperfSpawn(e.to_string())),
            }
        };
        let stdout = reader.join().unwrap_or_default();
        // iperf3 exits non-zero on failure but still prints a JSON `error` we can report.
        if !status.success() && stdout.trim().is_empty() {
            return Err(SpeedError::Iperf(format!("iperf3 exited with {status} and no output")));
        }
        Ok(stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures follow the documented `iperf3 --json` layout; values are invented.
    const TCP_JSON: &str = r#"{
      "start": {"version": "iperf 3.12"},
      "intervals": [],
      "end": {
        "streams": [],
        "sum_sent": {"start":0,"end":10,"seconds":10,"bytes":125000000,"bits_per_second":100000000.0,"retransmits":3},
        "sum_received": {"start":0,"end":10,"seconds":10,"bytes":123750000,"bits_per_second":99000000.0}
      }
    }"#;

    const UDP_JSON: &str = r#"{
      "start": {"version": "iperf 3.12"},
      "end": {
        "streams": [],
        "sum": {"start":0,"end":10,"seconds":10,"bytes":6250000,"bits_per_second":5000000.0,
                "jitter_ms":0.482,"lost_packets":12,"packets":4800,"lost_percent":0.25,"out_of_order":0}
      }
    }"#;

    #[test]
    fn tcp_uses_receiver_side_total_in_mbps() {
        let o = parse_report(TCP_JSON, Protocol::Tcp).unwrap();
        assert_eq!(o.mbps, 99.0);
        assert_eq!((o.jitter_ms, o.loss_pct), (None, None));
    }

    #[test]
    fn tcp_falls_back_to_sent_when_received_is_absent() {
        let j = r#"{"end":{"sum_sent":{"bits_per_second":8000000.0}}}"#;
        assert_eq!(parse_report(j, Protocol::Tcp).unwrap().mbps, 8.0);
    }

    #[test]
    fn udp_carries_jitter_and_loss() {
        let o = parse_report(UDP_JSON, Protocol::Udp(UdpBitrate::Unlimited)).unwrap();
        assert_eq!(o.mbps, 5.0);
        assert_eq!(o.jitter_ms, Some(0.482));
        assert_eq!(o.loss_pct, Some(0.25));
    }

    #[test]
    fn udp_zero_loss_is_a_measured_zero() {
        let j = r#"{"end":{"sum":{"bits_per_second":1000000.0,"jitter_ms":0.0,"lost_percent":0.0}}}"#;
        let o = parse_report(j, Protocol::Udp(UdpBitrate::Mbps(1.0))).unwrap();
        assert_eq!((o.jitter_ms, o.loss_pct), (Some(0.0), Some(0.0)));
    }

    #[test]
    fn server_error_is_reported_even_with_partial_data() {
        let j = r#"{"start":{},"intervals":[],"end":{},"error":"error - unable to connect to server: Connection refused"}"#;
        match parse_report(j, Protocol::Tcp) {
            Err(SpeedError::Iperf(m)) => assert!(m.contains("Connection refused")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn malformed_or_incomplete_output_is_an_error_not_a_zero() {
        assert!(matches!(parse_report("not json", Protocol::Tcp), Err(SpeedError::IperfOutput(_))));
        assert!(matches!(parse_report("{}", Protocol::Tcp), Err(SpeedError::IperfOutput(_))));
        assert!(matches!(parse_report(r#"{"end":{}}"#, Protocol::Tcp), Err(SpeedError::IperfOutput(_))));
        let neg = r#"{"end":{"sum_received":{"bits_per_second":-1.0}}}"#;
        assert!(matches!(parse_report(neg, Protocol::Tcp), Err(SpeedError::IperfOutput(_))));
    }

    #[test]
    fn version_banner() {
        assert_eq!(parse_version("iperf 3.12 (cJSON 1.7.15)\nLinux x\n").as_deref(), Some("3.12"));
        assert_eq!(parse_version("iperf 3.9\n").as_deref(), Some("3.9"));
        assert_eq!(parse_version("bash: iperf3: not found"), None);
        assert_eq!(parse_version(""), None);
    }

    fn args(cfg: &IperfConfig) -> Vec<String> {
        build_args(cfg).unwrap()
    }

    #[test]
    fn tcp_upload_args() {
        let mut c = IperfConfig::new("10.0.0.2", Protocol::Tcp, Direction::Upload);
        c.duration_secs = 5;
        c.streams = 4;
        assert_eq!(
            args(&c),
            ["--client", "10.0.0.2", "--port", "5201", "--time", "5", "--parallel", "4", "--json"]
        );
    }

    #[test]
    fn tcp_download_is_reverse_and_window_is_passed() {
        let mut c = IperfConfig::new("h", Protocol::Tcp, Direction::Download);
        c.port = 6000;
        c.window_bytes = Some(262_144);
        let a = args(&c);
        assert!(a.contains(&"--reverse".to_string()));
        assert!(a.windows(2).any(|w| w == ["--window", "262144"]));
        assert!(a.windows(2).any(|w| w == ["--port", "6000"]));
        assert!(!a.contains(&"--udp".to_string()));
    }

    #[test]
    fn udp_args_carry_an_explicit_bitrate() {
        let c = IperfConfig::new("h", Protocol::Udp(UdpBitrate::Mbps(25.5)), Direction::Upload);
        let a = args(&c);
        assert!(a.contains(&"--udp".to_string()));
        assert!(a.windows(2).any(|w| w == ["--bitrate", "25.500M"]));
        assert!(!a.contains(&"--reverse".to_string()));
        let c = IperfConfig::new("h", Protocol::Udp(UdpBitrate::Unlimited), Direction::Download);
        assert!(args(&c).windows(2).any(|w| w == ["--bitrate", "0"]));
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let ok = IperfConfig::new("h", Protocol::Tcp, Direction::Upload);
        assert!(build_args(&IperfConfig { host: " ".into(), ..ok.clone() }).is_err());
        assert!(build_args(&IperfConfig { duration_secs: 0, ..ok.clone() }).is_err());
        assert!(build_args(&IperfConfig { streams: 0, ..ok.clone() }).is_err());
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let c = IperfConfig { protocol: Protocol::Udp(UdpBitrate::Mbps(bad)), ..ok.clone() };
            assert!(build_args(&c).is_err(), "{bad}");
        }
    }

    #[test]
    fn reuse_tcp_result_picks_tcp_rate_or_fallback() {
        let fb = UdpBitrate::Mbps(10.0);
        assert_eq!(UdpBitrate::from_tcp_result(Some(87.5), fb), UdpBitrate::Mbps(87.5));
        assert_eq!(UdpBitrate::from_tcp_result(None, fb), fb);
        assert_eq!(UdpBitrate::from_tcp_result(Some(0.0), fb), fb);
        assert_eq!(UdpBitrate::from_tcp_result(Some(f64::NAN), fb), fb);
    }

    #[test]
    fn missing_binary_is_a_spawn_error() {
        let r = IperfRunner::with_binary("/nonexistent/iperf3-for-test");
        assert!(matches!(r.version(), Err(SpeedError::IperfSpawn(_))));
        let cfg = IperfConfig::new("h", Protocol::Tcp, Direction::Upload);
        assert!(matches!(r.run(&cfg, &CancelToken::new()), Err(SpeedError::IperfSpawn(_))));
    }

    #[cfg(unix)]
    mod process {
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        /// Writes an executable shell script standing in for iperf3 and returns its path.
        fn script(name: &str, body: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!("openspot-iperf-test-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("iperf3");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        /// Retries while the kernel reports the script as busy (ETXTBSY): another test thread's
        /// fork can briefly inherit the write handle we just closed. Test-only race.
        fn retry_busy<T>(mut f: impl FnMut() -> Result<T, SpeedError>) -> Result<T, SpeedError> {
            for _ in 0..20 {
                match f() {
                    Err(SpeedError::IperfSpawn(m)) if m.contains("busy") => thread::sleep(Duration::from_millis(50)),
                    other => return other,
                }
            }
            f()
        }

        #[test]
        fn run_parses_stdout_of_the_child() {
            let path = script("ok", &format!("cat <<'EOF'\n{TCP_JSON}\nEOF"));
            let cfg = IperfConfig::new("h", Protocol::Tcp, Direction::Download);
            let o = retry_busy(|| IperfRunner::with_binary(&path).run(&cfg, &CancelToken::new())).unwrap();
            assert_eq!(o.mbps, 99.0);
        }

        #[test]
        fn nonzero_exit_with_json_error_reports_the_error() {
            let path = script("err", "echo '{\"error\":\"the server is busy running a test\"}'; exit 1");
            let cfg = IperfConfig::new("h", Protocol::Tcp, Direction::Upload);
            let e = IperfRunner::with_binary(&path).run(&cfg, &CancelToken::new()).unwrap_err();
            assert!(matches!(e, SpeedError::Iperf(m) if m.contains("busy")));
        }

        #[test]
        fn nonzero_exit_without_output_is_an_error() {
            let path = script("silent", "exit 3");
            let cfg = IperfConfig::new("h", Protocol::Tcp, Direction::Upload);
            assert!(matches!(IperfRunner::with_binary(&path).run(&cfg, &CancelToken::new()), Err(SpeedError::Iperf(_))));
        }

        #[test]
        fn version_probe_reads_the_banner() {
            let path = script("ver", "echo 'iperf 3.16 (cJSON 1.7.15)'");
            assert_eq!(IperfRunner::with_binary(&path).version().unwrap(), "3.16");
        }

        #[test]
        fn cancel_kills_a_hung_child_promptly() {
            let path = script("hang", "exec sleep 30");
            let cfg = IperfConfig::new("h", Protocol::Tcp, Direction::Upload);
            let cancel = CancelToken::new();
            let c2 = cancel.clone();
            let t = thread::spawn(move || {
                thread::sleep(Duration::from_millis(200));
                c2.cancel();
            });
            let started = Instant::now();
            let r = retry_busy(|| IperfRunner::with_binary(&path).run(&cfg, &cancel));
            t.join().unwrap();
            assert!(matches!(r, Err(SpeedError::Cancelled)));
            assert!(started.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn deadline_kills_a_child_that_outlives_the_test() {
            let path = script("slow", "exec sleep 30");
            let started = Instant::now();
            let r = retry_busy(|| IperfRunner::with_binary(&path).supervise(&[], Duration::from_millis(300), &CancelToken::new()));
            assert!(matches!(r, Err(SpeedError::Timeout(_))), "{r:?}");
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }
}
