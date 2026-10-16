//! Synthesis of the notification cues as mono PCM.
// Only the Windows backend plays these samples; elsewhere the tests are the sole user.
#![cfg_attr(not(windows), allow(dead_code))]

use std::f32::consts::TAU;

use super::Cue;

/// CD-quality rate; every sound card supports it and the cues need nothing higher.
pub const SAMPLE_RATE_HZ: u32 = 44_100;

/// Peak amplitude of a cue (full scale is 1.0). Quiet enough not to startle in an office.
const VOLUME: f32 = 0.25;

/// Fade at each end of a note so it starts and stops without an audible click.
const FADE_SECS: f32 = 0.008;

/// One tone of a cue: frequency in Hz and length in seconds.
type Note = (f32, f32);

/// Two rising notes read as "something new".
const NEW_NETWORK_NOTES: [Note; 2] = [(880.0, 0.09), (1175.0, 0.12)];

/// One short high note: a quiet "got it" that can repeat every few seconds while surveying.
const POINT_RECORDED_NOTES: [Note; 1] = [(1047.0, 0.1)];

/// Two falling low notes read as "that did not work".
const SCAN_FAILED_NOTES: [Note; 2] = [(440.0, 0.12), (330.0, 0.2)];

fn notes(cue: Cue) -> &'static [Note] {
    match cue {
        Cue::NewNetwork => &NEW_NETWORK_NOTES,
        Cue::PointRecorded => &POINT_RECORDED_NOTES,
        Cue::ScanFailed => &SCAN_FAILED_NOTES,
    }
}

/// Mono PCM samples in -1.0..=1.0 for `cue` at [`SAMPLE_RATE_HZ`].
pub fn synthesize(cue: Cue) -> Vec<f32> {
    let rate = SAMPLE_RATE_HZ as f32;
    let mut out = Vec::new();
    for &(freq, secs) in notes(cue) {
        let count = (secs * rate) as usize;
        let fade = (FADE_SECS * rate) as usize;
        for i in 0..count {
            let edge = i.min(count - 1 - i);
            let envelope = if edge < fade { edge as f32 / fade as f32 } else { 1.0 };
            out.push(VOLUME * envelope * (TAU * freq * i as f32 / rate).sin());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signal power at `freq` via the Goertzel algorithm, normalised by length.
    fn tone_power(samples: &[f32], freq: f32) -> f32 {
        let w = TAU * freq / SAMPLE_RATE_HZ as f32;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &x in samples {
            let s = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2) / (samples.len() as f32).powi(2)
    }

    #[test]
    fn cue_length_matches_its_notes() {
        let total: f32 = NEW_NETWORK_NOTES.iter().map(|n| n.1).sum();
        let samples = synthesize(Cue::NewNetwork);
        let expected = (total * SAMPLE_RATE_HZ as f32) as usize;
        assert!(samples.len().abs_diff(expected) <= NEW_NETWORK_NOTES.len(), "{} vs {expected}", samples.len());
    }

    #[test]
    fn samples_stay_within_the_volume_limit_and_are_not_silent() {
        let samples = synthesize(Cue::NewNetwork);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= VOLUME + 1e-6, "peak {peak}");
        assert!(peak > VOLUME * 0.9, "cue should reach near its peak, got {peak}");
    }

    #[test]
    fn notes_start_and_end_at_silence_so_they_do_not_click() {
        let samples = synthesize(Cue::NewNetwork);
        assert!(samples[0].abs() < 1e-6);
        assert!(samples[samples.len() - 1].abs() < VOLUME * 0.05);
        // Also at the join between the two notes.
        let first_len = (NEW_NETWORK_NOTES[0].1 * SAMPLE_RATE_HZ as f32) as usize;
        assert!(samples[first_len - 1].abs() < VOLUME * 0.05 && samples[first_len].abs() < 1e-6);
    }

    #[test]
    fn each_note_contains_its_own_frequency_and_not_the_other() {
        let samples = synthesize(Cue::NewNetwork);
        let first_len = (NEW_NETWORK_NOTES[0].1 * SAMPLE_RATE_HZ as f32) as usize;
        let (a, b) = samples.split_at(first_len);
        let (fa, fb) = (NEW_NETWORK_NOTES[0].0, NEW_NETWORK_NOTES[1].0);
        assert!(tone_power(a, fa) > 20.0 * tone_power(a, fb), "first note should be {fa} Hz");
        assert!(tone_power(b, fb) > 20.0 * tone_power(b, fa), "second note should be {fb} Hz");
    }

    const ALL_CUES: [Cue; 3] = [Cue::NewNetwork, Cue::PointRecorded, Cue::ScanFailed];

    #[test]
    fn every_cue_is_audible_bounded_and_click_free() {
        for cue in ALL_CUES {
            let samples = synthesize(cue);
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > VOLUME * 0.9 && peak <= VOLUME + 1e-6, "{cue:?} peak {peak}");
            assert!(samples[0].abs() < 1e-6, "{cue:?} starts at silence");
            assert!(samples[samples.len() - 1].abs() < VOLUME * 0.05, "{cue:?} ends near silence");
        }
    }

    #[test]
    fn success_and_failure_cues_are_distinguishable_by_pitch() {
        let ok = synthesize(Cue::PointRecorded);
        let bad = synthesize(Cue::ScanFailed);
        let (ok_hz, bad_hz) = (POINT_RECORDED_NOTES[0].0, SCAN_FAILED_NOTES[0].0);
        assert!(tone_power(&ok, ok_hz) > 20.0 * tone_power(&ok, bad_hz));
        let first_len = (SCAN_FAILED_NOTES[0].1 * SAMPLE_RATE_HZ as f32) as usize;
        assert!(tone_power(&bad[..first_len], bad_hz) > 20.0 * tone_power(&bad[..first_len], ok_hz));
        assert!(SCAN_FAILED_NOTES[0].0 > SCAN_FAILED_NOTES[1].0, "failure falls");
    }
}
