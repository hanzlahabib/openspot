//! Active scan (speed tests at survey points): preferences, per-point job execution, and the
//! survey-side state. The egui parts live in `survey/ui/active_scan.rs`.

mod prefs;
mod run;

use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;

use openspot_model::SpeedResult;
use openspot_speedtest::CancelToken;

pub use prefs::ActiveScanPrefs;
pub use run::{ActiveScanJob, run_job, test_connection};

use crate::worker::Command;

/// Outcome of the settings dialog's connection test.
#[derive(Debug, Default)]
pub enum ConnectionTest {
    #[default]
    Idle,
    Running(Receiver<Vec<String>>),
    Done(Vec<String>),
}

/// Survey-side state of the active scan feature.
#[derive(Debug, Default)]
pub struct ActiveScanState {
    pub prefs: ActiveScanPrefs,
    pub settings_open: bool,
    /// Latest progress line from the worker while a point is being measured.
    pub progress: Option<String>,
    /// Token of the job in flight; the Cancel button fires it.
    pub cancel: CancelToken,
    pub test: ConnectionTest,
    /// Editable text form of `prefs.ssids` (one per line).
    pub ssids_text: String,
}

impl ActiveScanState {
    pub fn apply_prefs(&mut self, prefs: &ActiveScanPrefs) {
        self.prefs = prefs.clone().sanitized();
        self.ssids_text = self.prefs.ssids.join("\n");
    }

    /// The worker command for a click at `(x, y)`: a plain passive scan, or a passive scan
    /// followed by the configured tests. `Err` carries what the user must fix first.
    pub fn command_for(&mut self, x: f64, y: f64) -> Result<Command, String> {
        if !self.prefs.enabled {
            return Ok(Command::SurveyScan { x, y });
        }
        self.cancel = CancelToken::new();
        let job = ActiveScanJob::from_prefs(&self.prefs, self.cancel.clone())?;
        self.progress = Some("Passive scan…".into());
        Ok(Command::SurveyScanActive { x, y, job: Box::new(job) })
    }

    /// Starts the connection test on a helper thread (the worker may be mid-scan).
    pub fn start_test(&mut self, repaint: impl Fn() + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        let prefs = self.prefs.clone();
        thread::spawn(move || {
            // A send error only means the dialog was closed; the result is not wanted then.
            let _ = tx.send(test_connection(&prefs, &CancelToken::new()));
            repaint();
        });
        self.test = ConnectionTest::Running(rx);
    }

    /// Collects a finished connection test, if any.
    pub fn poll_test(&mut self) {
        if let ConnectionTest::Running(rx) = &self.test {
            match rx.try_recv() {
                Ok(lines) => self.test = ConnectionTest::Done(lines),
                Err(TryRecvError::Disconnected) => self.test = ConnectionTest::Done(vec!["connection test failed to run".into()]),
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}

/// One status-bar sentence about how an active scan went, `None` when everything worked.
pub fn summarize_failures(results: &[SpeedResult]) -> Option<String> {
    let errors: Vec<&str> = results.iter().flat_map(|r| r.errors.iter().map(String::as_str)).collect();
    let first = errors.first()?;
    Some(if errors.len() == 1 {
        format!("active scan: {first}")
    } else {
        format!("active scan: {first} (+{} more)", errors.len() - 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured() -> ActiveScanState {
        let mut s = ActiveScanState::default();
        s.apply_prefs(&ActiveScanPrefs { enabled: true, iperf_host: "h".into(), ssids: vec!["A".into()], ..ActiveScanPrefs::default() });
        s
    }

    #[test]
    fn disabled_gives_the_plain_passive_command() {
        let mut s = ActiveScanState::default();
        assert!(matches!(s.command_for(1.0, 2.0), Ok(Command::SurveyScan { x, y }) if (x, y) == (1.0, 2.0)));
        assert!(s.progress.is_none());
    }

    #[test]
    fn enabled_builds_a_job_from_the_prefs() {
        let mut s = configured();
        match s.command_for(1.0, 2.0).unwrap() {
            Command::SurveyScanActive { x, y, job } => {
                assert_eq!((x, y), (1.0, 2.0));
                assert_eq!(job.ssids, ["A"]);
                assert_eq!(job.suite.stages().len(), 2);
                assert!(!job.cancel.is_cancelled());
            }
            _ => panic!("expected an active scan command"),
        }
        assert!(s.progress.is_some());
    }

    #[test]
    fn each_job_gets_a_fresh_cancel_token() {
        let mut s = configured();
        s.command_for(0.0, 0.0).unwrap();
        s.cancel.cancel();
        let Command::SurveyScanActive { job, .. } = s.command_for(0.0, 0.0).unwrap() else { panic!() };
        assert!(!job.cancel.is_cancelled(), "a cancelled earlier job must not poison the next");
    }

    #[test]
    fn unconfigured_enabled_scan_explains_what_to_fix() {
        let mut s = ActiveScanState::default();
        s.prefs.enabled = true;
        let err = s.command_for(0.0, 0.0).unwrap_err();
        assert!(err.contains("iperf3 server"), "{err}");
        assert!(s.progress.is_none(), "no spinner text for a scan that never started");
    }

    #[test]
    fn failure_summary() {
        let ok = SpeedResult::new("a");
        assert_eq!(summarize_failures(&[ok.clone()]), None);
        let bad = |m: &str| SpeedResult { errors: vec![m.into()], ..SpeedResult::new("b") };
        assert_eq!(summarize_failures(&[ok.clone(), bad("x")]).unwrap(), "active scan: x");
        assert_eq!(summarize_failures(&[bad("x"), bad("y"), bad("z")]).unwrap(), "active scan: x (+2 more)");
    }
}
