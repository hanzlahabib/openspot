//! Background generation of a report (and optional per-AP heatmaps) with progress and cancel.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use openspot_model::Project;
use openspot_report::{Observer, ReportBranding, ReportData, ReportError, ReportOptions};
use openspot_wlan::MacAddress;

/// Everything the worker thread needs, owned so the UI keeps editing the project meanwhile.
pub struct JobRequest {
    pub project: Project,
    pub aliases: BTreeMap<MacAddress, String>,
    pub branding: ReportBranding,
    pub options: ReportOptions,
    pub pdf_path: PathBuf,
    /// Also write one heatmap per access point into this folder.
    pub per_ap_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub enum JobEvent {
    Progress { phase: &'static str, done: usize, total: usize, step: String },
    Finished(Result<JobOutput, String>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct JobOutput {
    pub pdf: PathBuf,
    pub heatmaps_written: usize,
    pub skipped_zones: Vec<String>,
}

#[derive(Debug)]
pub struct Progress {
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
    pub step: String,
}

/// Handle owned by the UI while a job runs.
#[derive(Debug)]
pub struct Job {
    cancel: Arc<AtomicBool>,
    events: Receiver<JobEvent>,
    pub progress: Progress,
    thread: Option<JoinHandle<()>>,
}

impl Job {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Returns the final result once the job is over; otherwise updates `progress`.
    pub fn poll(&mut self) -> Option<Result<JobOutput, String>> {
        while let Ok(event) = self.events.try_recv() {
            match event {
                JobEvent::Progress { phase, done, total, step } => self.progress = Progress { phase, done, total, step },
                JobEvent::Finished(result) => {
                    if let Some(t) = self.thread.take() {
                        // The thread has already sent its last event; this only reaps it.
                        let _ = t.join();
                    }
                    return Some(result);
                }
            }
        }
        None
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Dropping the handle (e.g. app exit) must not leave a worker running to completion.
        self.cancel();
    }
}

struct Reporter {
    phase: &'static str,
    cancel: Arc<AtomicBool>,
    tx: Sender<JobEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Observer for Reporter {
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn progress(&self, done: usize, total: usize, step: &str) {
        // A closed receiver means the UI is gone; the cancel flag stops the work.
        let _ = self.tx.send(JobEvent::Progress { phase: self.phase, done, total, step: step.to_owned() });
        (self.wake)();
    }
}

/// Starts the job on its own thread. `wake` is called after each event so an idle UI repaints.
pub fn spawn(request: JobRequest, wake: impl Fn() + Send + Sync + 'static) -> Job {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, events) = mpsc::channel();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
    let thread = {
        let (cancel, tx, wake) = (cancel.clone(), tx.clone(), wake.clone());
        std::thread::spawn(move || {
            let result = run(&request, &cancel, &tx, &wake).map_err(|e| match e {
                ReportError::Cancelled => "Export cancelled".to_owned(),
                other => other.to_string(),
            });
            let _ = tx.send(JobEvent::Finished(result));
            wake();
        })
    };
    Job { cancel, events, progress: Progress { phase: "Starting", done: 0, total: 1, step: String::new() }, thread: Some(thread) }
}

fn run(request: &JobRequest, cancel: &Arc<AtomicBool>, tx: &Sender<JobEvent>, wake: &Arc<dyn Fn() + Send + Sync>) -> Result<JobOutput, ReportError> {
    let reporter = |phase| Reporter { phase, cancel: cancel.clone(), tx: tx.clone(), wake: wake.clone() };
    let logo = request.branding.load_logo()?;
    let data = ReportData { project: &request.project, aliases: &request.aliases, branding: &request.branding, logo: logo.as_deref() };
    let pdf = openspot_report::generate_pdf(&data, &request.options, &reporter("Report"))?;
    write_atomically(&request.pdf_path, &pdf)?;

    let (mut heatmaps_written, mut skipped_zones) = (0, Vec::new());
    if let Some(dir) = &request.per_ap_dir {
        let out = openspot_report::export_per_ap_heatmaps(&request.project, &request.aliases, &request.options, dir, &reporter("Heatmaps"))?;
        heatmaps_written = out.written.len();
        skipped_zones = out.skipped_zones;
    }
    Ok(JobOutput { pdf: request.pdf_path.clone(), heatmaps_written, skipped_zones })
}

/// Writes next to the target and renames, so a crash never leaves half a PDF under the
/// final name.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), ReportError> {
    let io = |source| ReportError::Io { path: path.to_owned(), source };
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".partial");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(|source| {
        // Best effort: the rename error is the one worth reporting.
        let _ = std::fs::remove_file(&tmp);
        ReportError::Io { path: path.to_owned(), source }
    })
}

/// Maps every BSSID in `project` to its alias, from the app's alias store.
pub fn alias_map(project: &Project, aliases: &crate::aliases::Aliases) -> BTreeMap<MacAddress, String> {
    project
        .zones
        .iter()
        .flat_map(|z| &z.snapshots)
        .flat_map(|s| &s.points)
        .flat_map(|p| &p.access_points)
        .filter_map(|ap| aliases.get(&ap.bssid).map(|a| (ap.bssid, a.to_owned())))
        .collect()
}

/// Opens `path` with the system's default application.
pub fn open_with_default_app(path: &Path) -> std::io::Result<()> {
    use std::process::Command;
    let mut cmd = if cfg!(target_os = "windows") {
        // `explorer <file>` opens the file in its default handler without shell quoting rules.
        Command::new("explorer")
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    cmd.arg(path).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use openspot_model::{MapImage, ScanPoint};
    use openspot_wlan::AccessPoint;

    use super::*;

    fn project() -> Project {
        let mut p = Project::new("Job test");
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(120, 80, image::Rgba([240, 240, 240, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        p.zones[0].map = Some(MapImage { original_name: "m.png".into(), data: png.into_inner(), width: 120, height: 80 });
        for (i, (x, y)) in [(20.0, 20.0), (100.0, 20.0), (60.0, 60.0)].into_iter().enumerate() {
            let ap = AccessPoint::from_beacon("Net".into(), MacAddress([0xF4, 0xF2, 0x6D, 0, 0, 1]), -50 - 10 * i as i32, 2_437_000, true, 100, vec![]);
            p.zones[0].snapshots[0].points.push(ScanPoint::new(x, y, 1_790_000_000, vec![ap]));
        }
        p
    }

    fn request(dir: &Path, per_ap: bool) -> JobRequest {
        JobRequest {
            project: project(),
            aliases: BTreeMap::new(),
            branding: ReportBranding::default(),
            options: ReportOptions::quick(1_792_152_000),
            pdf_path: dir.join("out.pdf"),
            per_ap_dir: per_ap.then(|| dir.join("heatmaps")),
        }
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openspot-job-test-{}", openspot_model::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait(job: &mut Job) -> Result<JobOutput, String> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(r) = job.poll() {
                return r;
            }
            assert!(Instant::now() < deadline, "job did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn job_writes_pdf_and_heatmaps_and_reports_progress() {
        let dir = scratch();
        let mut job = spawn(request(&dir, true), || {});
        let out = wait(&mut job).unwrap();
        assert_eq!(out.pdf, dir.join("out.pdf"));
        assert_eq!(out.heatmaps_written, 1);
        let bytes = std::fs::read(&out.pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF-") && bytes.len() > 1000);
        assert_eq!(std::fs::read_dir(dir.join("heatmaps")).unwrap().count(), 1);
        assert!(!dir.join("out.pdf.partial").exists());
        assert_eq!(job.progress.phase, "Heatmaps");
        assert_eq!(job.progress.done, job.progress.total);
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn cancelling_before_the_work_starts_writes_nothing() {
        let dir = scratch();
        let mut req = request(&dir, true);
        // Plenty of work so the cancel flag is certain to be seen.
        for _ in 0..30 {
            let zone = req.project.zones[0].clone();
            req.project.zones.push(zone);
        }
        let mut job = spawn(req, || {});
        job.cancel();
        let err = wait(&mut job).unwrap_err();
        assert_eq!(err, "Export cancelled");
        assert!(!dir.join("out.pdf").exists());
        assert!(!dir.join("heatmaps").exists());
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn unwritable_target_reports_the_path() {
        let dir = scratch();
        let mut req = request(&dir, false);
        req.pdf_path = dir.join("missing-folder").join("out.pdf");
        let err = wait(&mut spawn(req, || {})).unwrap_err();
        assert!(err.contains("missing-folder"), "{err}");
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn missing_logo_is_reported_not_ignored() {
        let dir = scratch();
        let mut req = request(&dir, false);
        req.branding.logo_path = dir.join("nope.png").to_string_lossy().into_owned();
        let err = wait(&mut spawn(req, || {})).unwrap_err();
        assert!(err.contains("nope.png"), "{err}");
        assert!(!dir.join("out.pdf").exists());
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn alias_map_only_contains_aliased_aps_present_in_the_project() {
        let p = project();
        let mut aliases = crate::aliases::Aliases::default();
        aliases.set(&MacAddress([0xF4, 0xF2, 0x6D, 0, 0, 1]), "Lobby");
        aliases.set(&MacAddress([9; 6]), "Elsewhere");
        let map = alias_map(&p, &aliases);
        assert_eq!(map.len(), 1);
        assert_eq!(map[&MacAddress([0xF4, 0xF2, 0x6D, 0, 0, 1])], "Lobby");
    }
}
