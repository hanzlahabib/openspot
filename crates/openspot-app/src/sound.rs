//! Sound notifications: programmatically generated cues and a small player abstraction.
//!
//! No audio files are bundled: each cue is a few sine tones synthesised on demand, so there is
//! no third-party audio to license. Playback goes through the [`SoundPlayer`] trait so the
//! Survey code can reuse it and tests can use a fake.

use thiserror::Error;

mod synth;

/// Reasons a cue could not be played. Never fatal: callers show a notice and carry on.
#[derive(Debug, Error)]
pub enum SoundError {
    #[error("sound notifications are not supported on this platform yet")]
    Unsupported,
    #[error("could not open the audio output: {0}")]
    #[cfg_attr(not(windows), allow(dead_code, reason = "only the Windows backend opens a device"))]
    Device(String),
}

/// What happened, as far as the user should hear it. Add variants as features need them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// A network appeared that was not in the previous scan.
    NewNetwork,
    /// A survey point was measured and stored.
    PointRecorded,
    /// A survey measurement failed and nothing was stored.
    ScanFailed,
}

/// Anything that can make a cue audible.
pub trait SoundPlayer {
    /// Starts playing and returns without waiting for the sound to finish.
    fn play(&mut self, cue: Cue) -> Result<(), SoundError>;
}

/// Wraps a player so a broken audio setup is reported once instead of on every scan.
pub struct SoundNotifier {
    player: Box<dyn SoundPlayer>,
    failure: Option<String>,
}

impl std::fmt::Debug for SoundNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoundNotifier").field("failure", &self.failure).finish_non_exhaustive()
    }
}

impl SoundNotifier {
    pub fn new(player: Box<dyn SoundPlayer>) -> Self {
        Self { player, failure: None }
    }

    /// The platform's player.
    pub fn system() -> Self {
        Self::new(system_player())
    }

    /// Plays `cue`. Returns a message the first time playing fails; afterwards, until
    /// [`SoundNotifier::reset`], it stays silent without retrying.
    pub fn play(&mut self, cue: Cue) -> Option<String> {
        if self.failure.is_some() {
            return None;
        }
        match self.player.play(cue) {
            Ok(()) => None,
            Err(e) => {
                let message = format!("Sound notifications unavailable: {e}");
                self.failure = Some(message.clone());
                Some(message)
            }
        }
    }

    /// Forgets a previous failure so the next cue tries again (the user changed a setting
    /// or pressed "Test sound").
    pub fn reset(&mut self) {
        self.failure = None;
    }
}

#[cfg(windows)]
mod rodio_player {
    use std::num::NonZero;

    use rodio::buffer::SamplesBuffer;
    use rodio::{DeviceSinkBuilder, MixerDeviceSink};

    use super::synth::{SAMPLE_RATE_HZ, synthesize};
    use super::{Cue, SoundError, SoundPlayer};

    /// Plays through the default output device, opened on first use and kept open: the stream
    /// must outlive the sounds it plays.
    #[derive(Default)]
    pub struct RodioPlayer {
        sink: Option<MixerDeviceSink>,
    }

    impl std::fmt::Debug for RodioPlayer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RodioPlayer").field("open", &self.sink.is_some()).finish()
        }
    }

    impl SoundPlayer for RodioPlayer {
        fn play(&mut self, cue: Cue) -> Result<(), SoundError> {
            if self.sink.is_none() {
                let mut sink = DeviceSinkBuilder::from_default_device()
                    .and_then(DeviceSinkBuilder::open_stream)
                    .map_err(|e| SoundError::Device(e.to_string()))?;
                sink.log_on_drop(false);
                self.sink = Some(sink);
            }
            let Some(sink) = &self.sink else { return Err(SoundError::Device("no output stream".to_owned())) };
            let mono = NonZero::<u16>::MIN;
            let rate = NonZero::new(SAMPLE_RATE_HZ).ok_or(SoundError::Unsupported)?;
            sink.mixer().add(SamplesBuffer::new(mono, rate, synthesize(cue)));
            Ok(())
        }
    }
}

#[cfg(windows)]
fn system_player() -> Box<dyn SoundPlayer> {
    Box::<rodio_player::RodioPlayer>::default()
}

/// Audio output is only wired up on Windows so far (the same platforms the Wi-Fi backend
/// supports); elsewhere the preference reports why it cannot work.
#[cfg(not(windows))]
fn system_player() -> Box<dyn SoundPlayer> {
    struct Unavailable;
    impl SoundPlayer for Unavailable {
        fn play(&mut self, _cue: Cue) -> Result<(), SoundError> {
            Err(SoundError::Unsupported)
        }
    }
    Box::new(Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Recorder {
        calls: Rc<RefCell<Vec<Cue>>>,
        fail: bool,
    }

    impl SoundPlayer for Recorder {
        fn play(&mut self, cue: Cue) -> Result<(), SoundError> {
            self.calls.borrow_mut().push(cue);
            if self.fail { Err(SoundError::Device("no speakers".into())) } else { Ok(()) }
        }
    }

    #[test]
    fn notifier_forwards_cues_to_a_working_player() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut n = SoundNotifier::new(Box::new(Recorder { calls: calls.clone(), fail: false }));
        assert_eq!(n.play(Cue::NewNetwork), None);
        assert_eq!(n.play(Cue::NewNetwork), None);
        assert_eq!(calls.borrow().len(), 2);
    }

    #[test]
    fn notifier_reports_a_failure_once_then_stays_quiet_until_reset() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut n = SoundNotifier::new(Box::new(Recorder { calls: calls.clone(), fail: true }));
        let message = n.play(Cue::NewNetwork).expect("first failure is reported");
        assert!(message.contains("no speakers"), "{message}");
        assert_eq!(n.play(Cue::NewNetwork), None, "no repeated notices");
        assert_eq!(calls.borrow().len(), 1, "and no repeated attempts");
        n.reset();
        assert!(n.play(Cue::NewNetwork).is_some(), "reset allows another attempt");
        assert_eq!(calls.borrow().len(), 2);
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_platforms_report_a_typed_error_instead_of_crashing() {
        let mut n = SoundNotifier::system();
        let message = n.play(Cue::NewNetwork).expect("unsupported is reported");
        assert!(message.contains("not supported"), "{message}");
    }
}
