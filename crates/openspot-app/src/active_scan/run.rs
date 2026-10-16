//! Worker-side execution of one active scan at a survey point, and the settings dialog's
//! connection test. UI-free so it can be tested with fake providers.

use std::thread;
use std::time::{Duration, Instant};

use openspot_model::SpeedResult;
use openspot_speedtest::http;
use openspot_speedtest::iperf::IperfRunner;
use openspot_speedtest::suite::{self, SuiteConfig};
use openspot_speedtest::CancelToken;
use openspot_wlan::{Adapter, ConnectTarget, ConnectionGuard, DEFAULT_CONNECT_TIMEOUT, WlanProvider};

use super::prefs::ActiveScanPrefs;

/// How often a cancellable wait re-checks the cancel flag.
const WAIT_SLICE: Duration = Duration::from_millis(100);
/// Bytes fetched by the HTTP connection test: enough to prove the URL serves data, small
/// enough to finish instantly.
const HTTP_PROBE_BYTES: u64 = 256 * 1024;

/// Everything the worker needs to run the tests for one point.
#[derive(Debug)]
pub struct ActiveScanJob {
    pub suite: SuiteConfig,
    /// Saved profile names to test one after another; empty = test the current connection.
    pub ssids: Vec<String>,
    pub delay: Duration,
    pub connect_timeout: Duration,
    pub cancel: CancelToken,
}

impl ActiveScanJob {
    pub fn from_prefs(prefs: &ActiveScanPrefs, cancel: CancelToken) -> Result<Self, String> {
        let prefs = prefs.clone().sanitized();
        Ok(Self {
            suite: prefs.to_suite()?,
            ssids: prefs.ssids.clone(),
            delay: Duration::from_secs(u64::from(prefs.delay_secs)),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            cancel,
        })
    }
}

/// Sleeps for `total` unless cancelled; returns false when cancelled.
fn wait(total: Duration, cancel: &CancelToken) -> bool {
    let end = Instant::now() + total;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if cancel.is_cancelled() {
            return false;
        }
        thread::sleep(left.min(WAIT_SLICE));
    }
    !cancel.is_cancelled()
}

fn failed(ssid: &str, error: String) -> SpeedResult {
    SpeedResult { errors: vec![error], ..SpeedResult::new(ssid) }
}

fn measure(ssid: &str, job: &ActiveScanJob, progress: &mut dyn FnMut(String)) -> SpeedResult {
    if !wait(job.delay, &job.cancel) {
        return failed(ssid, "cancelled".into());
    }
    let label = if ssid.is_empty() { "current network".to_owned() } else { ssid.to_owned() };
    suite::run(ssid, &job.suite, &job.cancel, &mut |stage| progress(format!("{label}: {}…", stage.label())))
}

/// Runs the job on `adapter`. Networks are visited in order; the adapter's original
/// connection is restored afterwards even when a network fails or the job is cancelled.
/// One result per attempted network, failures carried in `errors` rather than dropped.
pub fn run_job(
    provider: &dyn WlanProvider,
    adapter: &Adapter,
    job: &ActiveScanJob,
    progress: &mut dyn FnMut(String),
) -> Vec<SpeedResult> {
    if job.ssids.is_empty() {
        // Label with whatever the adapter is on, when the backend can tell us.
        let current = provider.connected_profile(adapter).ok().flatten().unwrap_or_default();
        return vec![measure(&current, job, progress)];
    }
    let mut guard = match ConnectionGuard::begin(provider, adapter) {
        Ok(g) => g,
        Err(e) => {
            let msg = format!("cannot switch networks: {e}");
            return job.ssids.iter().map(|s| failed(s, msg.clone())).collect();
        }
    };
    let mut results = Vec::new();
    for ssid in &job.ssids {
        if job.cancel.is_cancelled() {
            break;
        }
        progress(format!("Connecting to {ssid}…"));
        match guard.connect(&ConnectTarget::SavedProfile(ssid.clone()), job.connect_timeout) {
            Ok(()) => results.push(measure(ssid, job, progress)),
            Err(e) => results.push(failed(ssid, format!("could not connect: {e}"))),
        }
    }
    progress("Restoring the previous connection…".into());
    if let Err(e) = guard.restore() {
        let msg = format!("could not restore the previous connection: {e}");
        match results.last_mut() {
            Some(last) => last.errors.push(msg),
            None => results.push(failed("", msg)),
        }
    }
    results
}

/// Settings-dialog "Test connection": checks each selected transport once and returns one
/// report line per check. Blocking; call from a helper thread.
pub fn test_connection(prefs: &ActiveScanPrefs, cancel: &CancelToken) -> Vec<String> {
    let p = prefs.clone().sanitized();
    let mut lines = Vec::new();
    if let Err(e) = p.to_suite() {
        return vec![e];
    }
    if p.use_http {
        for (what, url) in [("download", &p.http_download_url), ("upload", &p.http_upload_url)] {
            let cfg = p.http_config(url, 1);
            // Only a download can be probed without sending a payload to the server.
            let line = if what == "download" {
                let probe = openspot_speedtest::http::HttpConfig { payload_bytes: HTTP_PROBE_BYTES, ..cfg };
                match http::download(&probe, cancel) {
                    Ok(m) => format!("HTTP download OK ({:.1} Mbps over {} bytes)", m.mbps, m.bytes),
                    Err(e) => format!("HTTP download FAILED: {e}"),
                }
            } else {
                match http::upload(&openspot_speedtest::http::HttpConfig { payload_bytes: HTTP_PROBE_BYTES, ..cfg }, cancel) {
                    Ok(m) => format!("HTTP upload OK ({:.1} Mbps over {} bytes)", m.mbps, m.bytes),
                    Err(e) => format!("HTTP upload FAILED: {e}"),
                }
            };
            lines.push(line);
        }
    }
    if p.use_iperf_tcp || p.use_iperf_udp {
        let runner = if p.iperf_binary.is_empty() { IperfRunner::default() } else { IperfRunner::with_binary(&p.iperf_binary) };
        match runner.version() {
            Ok(v) => {
                lines.push(format!("iperf3 {v} found"));
                match runner.test_connection(p.iperf_host.trim(), p.iperf_port, cancel) {
                    Ok(o) => lines.push(format!("iperf3 server reachable ({:.1} Mbps in a 1 s test)", o.mbps)),
                    Err(e) => lines.push(format!("iperf3 server FAILED: {e}")),
                }
            }
            Err(e) => lines.push(format!("iperf3 FAILED: {e}")),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use openspot_wlan::{AccessPoint, ScanOptions, WlanError};

    use super::*;

    /// Provider that records connect/disconnect calls and can fail one profile.
    struct Switcher {
        current: Mutex<Option<String>>,
        log: Mutex<Vec<String>>,
        unsupported: bool,
        fail: Option<&'static str>,
    }

    impl Switcher {
        fn new(current: Option<&str>) -> Self {
            Self { current: Mutex::new(current.map(str::to_owned)), log: Mutex::default(), unsupported: false, fail: None }
        }
    }

    impl WlanProvider for Switcher {
        fn adapters(&self) -> Result<Vec<Adapter>, WlanError> {
            Ok(vec![])
        }
        fn scan(&self, _: &Adapter, _: &ScanOptions) -> Result<Vec<AccessPoint>, WlanError> {
            Ok(vec![])
        }
        fn connected_profile(&self, _: &Adapter) -> Result<Option<String>, WlanError> {
            if self.unsupported {
                return Err(WlanError::Unsupported("connection management"));
            }
            Ok(self.current.lock().unwrap().clone())
        }
        fn connect_profile(&self, _: &Adapter, name: &str, _: Duration) -> Result<(), WlanError> {
            self.log.lock().unwrap().push(format!("connect {name}"));
            if self.fail == Some(name) {
                return Err(WlanError::ConnectTimeout);
            }
            *self.current.lock().unwrap() = Some(name.into());
            Ok(())
        }
        fn disconnect(&self, _: &Adapter) -> Result<(), WlanError> {
            self.log.lock().unwrap().push("disconnect".into());
            *self.current.lock().unwrap() = None;
            Ok(())
        }
    }

    fn adapter() -> Adapter {
        Adapter { id: "a".into(), description: String::new() }
    }

    /// Job whose HTTP tests fail instantly (nothing listens on port 1), so no network is needed.
    fn job(ssids: &[&str]) -> ActiveScanJob {
        let prefs = ActiveScanPrefs {
            use_http: true,
            use_iperf_tcp: false,
            http_download_url: "http://127.0.0.1:1/d".into(),
            http_upload_url: "http://127.0.0.1:1/u".into(),
            delay_secs: 0,
            ssids: ssids.iter().map(|s| (*s).to_owned()).collect(),
            ..ActiveScanPrefs::default()
        };
        ActiveScanJob::from_prefs(&prefs, CancelToken::new()).unwrap()
    }

    #[test]
    fn visits_each_network_then_restores_the_original() {
        let p = Switcher::new(Some("Home"));
        let mut progress = Vec::new();
        let r = run_job(&p, &adapter(), &job(&["A", "B"]), &mut |m| progress.push(m));
        assert_eq!(r.iter().map(|x| x.ssid.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(*p.log.lock().unwrap(), ["connect A", "connect B", "connect Home"]);
        assert_eq!(p.current.lock().unwrap().as_deref(), Some("Home"));
        assert!(progress.iter().any(|m| m.contains("Connecting to A")));
        assert!(progress.iter().any(|m| m.contains("HTTP download")));
        // Measurements failed (no server), and that is recorded, never zeroed.
        assert!(r[0].http_down_mbps.is_none() && !r[0].errors.is_empty());
    }

    #[test]
    fn a_network_that_cannot_be_joined_is_reported_and_the_rest_still_run() {
        let mut p = Switcher::new(None);
        p.fail = Some("A");
        let r = run_job(&p, &adapter(), &job(&["A", "B"]), &mut |_| {});
        assert!(r[0].errors[0].starts_with("could not connect"));
        assert_eq!(r[1].ssid, "B");
        assert_eq!(p.log.lock().unwrap().last().map(String::as_str), Some("disconnect"), "was offline before");
    }

    #[test]
    fn no_ssids_tests_the_current_connection_without_switching() {
        let p = Switcher::new(Some("Home"));
        let r = run_job(&p, &adapter(), &job(&[]), &mut |_| {});
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].ssid, "Home");
        assert!(p.log.lock().unwrap().is_empty());
    }

    #[test]
    fn unsupported_backend_refuses_to_switch_instead_of_measuring_the_wrong_network() {
        let mut p = Switcher::new(None);
        p.unsupported = true;
        let r = run_job(&p, &adapter(), &job(&["A", "B"]), &mut |_| {});
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|x| x.is_empty() && x.errors[0].contains("cannot switch networks")));
    }

    #[test]
    fn cancelling_before_the_first_network_still_restores_nothing_and_returns_nothing() {
        let p = Switcher::new(Some("Home"));
        let j = job(&["A"]);
        j.cancel.cancel();
        let r = run_job(&p, &adapter(), &j, &mut |_| {});
        assert!(r.is_empty());
        assert_eq!(*p.log.lock().unwrap(), ["connect Home"], "guard restores the (unchanged) connection");
    }

    #[test]
    fn cancellable_wait() {
        let c = CancelToken::new();
        assert!(wait(Duration::from_millis(30), &c));
        c.cancel();
        let t = Instant::now();
        assert!(!wait(Duration::from_secs(10), &c));
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn connection_test_reports_config_problems_and_missing_iperf() {
        let c = CancelToken::new();
        assert!(test_connection(&ActiveScanPrefs::default(), &c)[0].contains("iperf3 server"));
        let p = ActiveScanPrefs { iperf_host: "h".into(), iperf_binary: "/nonexistent/iperf3-x".into(), ..ActiveScanPrefs::default() };
        let lines = test_connection(&p, &c);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("iperf3 FAILED"), "{lines:?}");
    }
}
