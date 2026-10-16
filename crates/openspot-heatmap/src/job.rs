//! Background execution for heatmap computation (H-19): a cancellable job on its own
//! thread with progress, so rasterizing never blocks the UI.
//!
//! The compute functions take a [`Control`] and check it at row granularity; with
//! [`Control::NONE`] they run exactly as the plain synchronous API, so a background result
//! is identical to the synchronous one by construction.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// Marker returned when work stopped because it was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

/// Flag shared between the owner of a job and the job itself.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        // Relaxed is enough: the flag carries no data, only "stop soon".
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Completed fraction `0.0..=1.0`, readable from any thread.
#[derive(Debug, Clone, Default)]
pub struct Progress(Arc<AtomicU32>);

impl Progress {
    pub fn set(&self, fraction: f32) {
        self.0.store(fraction.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// What a computation consults while running: whether to stop, and where to report
/// progress. A `Control` can be narrowed to a slice of the whole with [`Control::scoped`],
/// so each stage reports its own 0..1 while the job shows overall progress.
#[derive(Debug, Clone, Copy)]
pub struct Control<'a> {
    cancel: Option<&'a CancelToken>,
    progress: Option<&'a Progress>,
    lo: f32,
    hi: f32,
}

impl Control<'static> {
    /// Never cancels, reports nowhere: the synchronous case.
    pub const NONE: Self = Self { cancel: None, progress: None, lo: 0.0, hi: 1.0 };
}

impl<'a> Control<'a> {
    pub fn new(cancel: &'a CancelToken, progress: &'a Progress) -> Self {
        Self { cancel: Some(cancel), progress: Some(progress), lo: 0.0, hi: 1.0 }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_some_and(CancelToken::is_cancelled)
    }

    /// Reports `local` (0..1 within this control's slice).
    pub fn report(&self, local: f32) {
        if let Some(p) = self.progress {
            p.set(self.lo + local.clamp(0.0, 1.0) * (self.hi - self.lo));
        }
    }

    /// A control covering `lo..hi` of this one's slice.
    pub fn scoped(&self, lo: f32, hi: f32) -> Self {
        let span = self.hi - self.lo;
        Self { lo: self.lo + lo * span, hi: self.lo + hi * span, ..*self }
    }

    /// `Err(Cancelled)` once cancellation was requested.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() { Err(Cancelled) } else { Ok(()) }
    }
}

/// Outcome of polling a [`Job`].
#[derive(Debug, PartialEq)]
pub enum JobPoll<T> {
    Pending,
    Done(T),
    Cancelled,
    /// The worker thread panicked or could not be started.
    Failed,
}

/// A computation running on its own thread. Dropping the job cancels it and waits for the
/// thread, so no worker outlives its owner.
#[derive(Debug)]
pub struct Job<T> {
    rx: Receiver<Result<T, Cancelled>>,
    cancel: CancelToken,
    progress: Progress,
    handle: Option<JoinHandle<()>>,
    finished: bool,
}

impl<T: Send + 'static> Job<T> {
    /// Starts `work` on a new thread named `name`.
    pub fn spawn(name: &str, work: impl FnOnce(&Control) -> Result<T, Cancelled> + Send + 'static) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let (cancel, progress) = (CancelToken::default(), Progress::default());
        let (c, p) = (cancel.clone(), progress.clone());
        let handle = thread::Builder::new().name(name.to_owned()).spawn(move || {
            let result = work(&Control::new(&c, &p));
            if result.is_ok() {
                p.set(1.0);
            }
            // The receiver is gone only if the job was dropped, in which case nobody wants it.
            let _ = tx.send(result);
        })?;
        Ok(Self { rx, cancel, progress, handle: Some(handle), finished: false })
    }
}

impl<T> Job<T> {
    pub fn progress(&self) -> f32 {
        self.progress.get()
    }

    /// Asks the worker to stop; it does so at its next checkpoint.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Non-blocking. Returns each terminal state once; polling afterwards reports `Failed`.
    pub fn poll(&mut self) -> JobPoll<T> {
        if self.finished {
            return JobPoll::Failed;
        }
        match self.rx.try_recv() {
            Ok(Ok(v)) => self.finish(JobPoll::Done(v)),
            Ok(Err(Cancelled)) => self.finish(JobPoll::Cancelled),
            Err(TryRecvError::Empty) => JobPoll::Pending,
            // Sender dropped without sending: the thread panicked.
            Err(TryRecvError::Disconnected) => self.finish(JobPoll::Failed),
        }
    }

    fn finish(&mut self, outcome: JobPoll<T>) -> JobPoll<T> {
        self.finished = true;
        self.reap();
        outcome
    }

    fn reap(&mut self) {
        if let Some(handle) = self.handle.take() {
            // A panic is already reported as `Failed`; the payload adds nothing.
            let _ = handle.join();
        }
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.reap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait<T>(job: &mut Job<T>) -> JobPoll<T> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match job.poll() {
                JobPoll::Pending if Instant::now() < deadline => thread::sleep(Duration::from_millis(2)),
                other => return other,
            }
        }
    }

    #[test]
    fn completes_with_value_and_full_progress() {
        let mut job = Job::spawn("t", |ctl| {
            ctl.report(0.5);
            Ok(42)
        })
        .unwrap();
        assert_eq!(wait(&mut job), JobPoll::Done(42));
        assert_eq!(job.progress(), 1.0);
        assert_eq!(job.poll(), JobPoll::Failed, "terminal state is reported once");
    }

    #[test]
    fn cancel_is_honoured_promptly() {
        let mut job: Job<()> = Job::spawn("t", |ctl| {
            loop {
                ctl.check()?;
                thread::sleep(Duration::from_millis(1));
            }
        })
        .unwrap();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(job.poll(), JobPoll::Pending);
        let start = Instant::now();
        job.cancel();
        assert_eq!(wait(&mut job), JobPoll::Cancelled);
        assert!(start.elapsed() < Duration::from_secs(1), "took {:?}", start.elapsed());
    }

    #[test]
    fn dropping_a_running_job_stops_and_joins_its_thread() {
        let witness = Arc::new(());
        let held = Arc::clone(&witness);
        let job: Job<()> = Job::spawn("t", move |ctl| {
            let _held = held; // lives exactly as long as the closure, i.e. the thread
            loop {
                ctl.check()?;
                thread::sleep(Duration::from_millis(1));
            }
        })
        .unwrap();
        assert_eq!(Arc::strong_count(&witness), 2, "worker is running");
        drop(job);
        assert_eq!(Arc::strong_count(&witness), 1, "worker finished before drop returned");
    }

    #[test]
    fn a_panicking_worker_is_reported_as_failed() {
        let mut job: Job<()> = Job::spawn("t", |_| panic!("boom")).unwrap();
        assert_eq!(wait(&mut job), JobPoll::Failed);
    }

    #[test]
    fn scoped_controls_map_progress_into_their_slice() {
        let (c, p) = (CancelToken::default(), Progress::default());
        let ctl = Control::new(&c, &p);
        let second_half = ctl.scoped(0.5, 1.0);
        second_half.report(0.0);
        assert_eq!(p.get(), 0.5);
        second_half.scoped(0.0, 0.5).report(1.0);
        assert_eq!(p.get(), 0.75);
        second_half.report(7.0);
        assert_eq!(p.get(), 1.0, "clamped");
        assert!(!ctl.is_cancelled());
        c.cancel();
        assert!(second_half.is_cancelled());
        assert_eq!(second_half.check(), Err(Cancelled));
        Control::NONE.report(0.3);
        assert!(Control::NONE.check().is_ok());
    }
}
