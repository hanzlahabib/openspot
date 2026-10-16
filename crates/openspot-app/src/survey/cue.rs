//! Which sound, if any, a finished survey measurement should make.

use crate::sound::Cue;

/// What a finished measurement came to, as far as sound is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanOutcome {
    /// The point was stored.
    Recorded,
    /// The scan failed; nothing was stored.
    Failed,
}

/// The cue for `outcome`, or `None` when sound is off.
///
/// During a path run a point is recorded every few seconds while the user is walking, so a
/// success beep each time would be noise; only failures (which pause the run) stay audible.
pub fn cue_for(enabled: bool, outcome: ScanOutcome, path_run_active: bool) -> Option<Cue> {
    if !enabled {
        return None;
    }
    match outcome {
        ScanOutcome::Recorded if path_run_active => None,
        ScanOutcome::Recorded => Some(Cue::PointRecorded),
        ScanOutcome::Failed => Some(Cue::ScanFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_is_opt_in() {
        assert_eq!(cue_for(false, ScanOutcome::Recorded, false), None);
        assert_eq!(cue_for(false, ScanOutcome::Failed, false), None);
    }

    #[test]
    fn outcomes_map_to_distinct_cues() {
        assert_eq!(cue_for(true, ScanOutcome::Recorded, false), Some(Cue::PointRecorded));
        assert_eq!(cue_for(true, ScanOutcome::Failed, false), Some(Cue::ScanFailed));
    }

    #[test]
    fn path_runs_beep_only_on_failure() {
        assert_eq!(cue_for(true, ScanOutcome::Recorded, true), None);
        assert_eq!(cue_for(true, ScanOutcome::Failed, true), Some(Cue::ScanFailed));
    }
}
