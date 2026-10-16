//! Channel numbers and widths offered per band when configuring a virtual AP.
//!
//! The value stored in a `Radio` is the *center* channel number of the occupied spectrum
//! (IEEE 802.11 numbering), which is what the engine converts to a frequency.

use openspot_model::Band;

/// Widths (MHz) selectable per band. 320 MHz (Wi-Fi 7) is not offered: the engine's
/// interference model and the survey side have no 320 MHz data to compare against yet.
pub fn widths(band: Band) -> &'static [u16] {
    match band {
        Band::Ghz2_4 => &[20, 40],
        Band::Ghz5 => &[20, 40, 80, 160],
        Band::Ghz6 => &[20, 40, 80, 160],
    }
}

const CH_24_20: &[u16] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
/// Centers of two bonded 20 MHz channels (primary + secondary 5 apart in numbering).
const CH_24_40: &[u16] = &[3, 4, 5, 6, 7, 8, 9, 10, 11];

const CH_5_20: &[u16] = &[36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 149, 153, 157, 161, 165];
const CH_5_40: &[u16] = &[38, 46, 54, 62, 102, 110, 118, 126, 134, 142, 151, 159];
const CH_5_80: &[u16] = &[42, 58, 106, 122, 138, 155];
const CH_5_160: &[u16] = &[50, 114];

const CH_6_20: [u16; 59] = steps(1, 4);
const CH_6_40: [u16; 29] = steps(3, 8);
const CH_6_80: [u16; 14] = steps(7, 16);
const CH_6_160: [u16; 7] = steps(15, 32);

/// `start, start + step, ...` for the 6 GHz U-NII-5..8 range (channels 1 to 233).
const fn steps<const N: usize>(start: u16, step: u16) -> [u16; N] {
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = start + step * i as u16;
        i += 1;
    }
    out
}

/// Center channel numbers valid for `band` at `width_mhz`; empty for an unsupported width.
pub fn channels(band: Band, width_mhz: u16) -> &'static [u16] {
    match (band, width_mhz) {
        (Band::Ghz2_4, 20) => CH_24_20,
        (Band::Ghz2_4, 40) => CH_24_40,
        (Band::Ghz5, 20) => CH_5_20,
        (Band::Ghz5, 40) => CH_5_40,
        (Band::Ghz5, 80) => CH_5_80,
        (Band::Ghz5, 160) => CH_5_160,
        (Band::Ghz6, 20) => &CH_6_20,
        (Band::Ghz6, 40) => &CH_6_40,
        (Band::Ghz6, 80) => &CH_6_80,
        (Band::Ghz6, 160) => &CH_6_160,
        _ => &[],
    }
}

/// The valid channel closest (by frequency) to `channel`, for keeping a radio sensible when
/// its width or band changes. `None` for an unsupported width.
pub fn nearest_channel(band: Band, width_mhz: u16, channel: u16) -> Option<u16> {
    let target = band.channel_center_mhz(channel);
    channels(band, width_mhz).iter().copied().min_by(|&a, &b| (band.channel_center_mhz(a) - target).abs().total_cmp(&(band.channel_center_mhz(b) - target).abs()))
}

pub fn is_valid(band: Band, width_mhz: u16, channel: u16) -> bool {
    channels(band, width_mhz).contains(&channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bonded center must be the mean of the 20 MHz channels it covers, and those
    /// must all exist: this ties the hard-coded wide tables to the 20 MHz plan.
    #[test]
    fn wide_centers_are_the_mean_of_their_constituent_20_mhz_channels() {
        for band in [Band::Ghz5, Band::Ghz6] {
            let base = channels(band, 20);
            for &width in &widths(band)[1..] {
                let n = usize::from(width / 20);
                for &center in channels(band, width) {
                    let members: Vec<u16> = (0..n).map(|i| (f64::from(center) + (i as f64 - (n as f64 - 1.0) / 2.0) * 4.0) as u16).collect();
                    assert!(members.iter().all(|m| base.contains(m)), "{band:?} {width} MHz center {center}: members {members:?}");
                    let mean = members.iter().map(|&m| f64::from(m)).sum::<f64>() / n as f64;
                    assert_eq!(mean, f64::from(center));
                }
            }
        }
    }

    #[test]
    fn known_channels_and_frequencies() {
        assert_eq!(channels(Band::Ghz6, 20).len(), 59);
        assert_eq!((channels(Band::Ghz6, 20)[0], channels(Band::Ghz6, 20)[58]), (1, 233));
        assert_eq!(channels(Band::Ghz6, 80), [7, 23, 39, 55, 71, 87, 103, 119, 135, 151, 167, 183, 199, 215]);
        assert_eq!(*channels(Band::Ghz6, 160).last().unwrap(), 207);
        assert_eq!(Band::Ghz5.channel_center_mhz(42), 5210.0, "80 MHz block 36-48");
        assert_eq!(Band::Ghz2_4.channel_center_mhz(*channels(Band::Ghz2_4, 20).last().unwrap()), 2472.0);
        assert!(channels(Band::Ghz2_4, 80).is_empty());
    }

    #[test]
    fn every_listed_channel_sits_inside_its_band() {
        let limits = |b: Band| match b {
            Band::Ghz2_4 => (2400.0, 2495.0),
            Band::Ghz5 => (5150.0, 5895.0),
            Band::Ghz6 => (5925.0, 7125.0),
        };
        for band in Band::ALL {
            for &w in widths(band) {
                assert!(!channels(band, w).is_empty());
                let (lo, hi) = limits(band);
                for &c in channels(band, w) {
                    let f = band.channel_center_mhz(c);
                    assert!(f - f64::from(w) / 2.0 >= lo - 1.0 && f + f64::from(w) / 2.0 <= hi + 1.0, "{band:?} {w} MHz ch {c} = {f} MHz");
                }
            }
        }
    }

    #[test]
    fn nearest_channel_follows_the_frequency() {
        assert_eq!(nearest_channel(Band::Ghz5, 80, 36), Some(42));
        assert_eq!(nearest_channel(Band::Ghz5, 80, 100), Some(106));
        assert_eq!(nearest_channel(Band::Ghz5, 20, 42), Some(40), "tie between 40 and 44 resolves to the lower");
        assert_eq!(nearest_channel(Band::Ghz2_4, 40, 1), Some(3));
        assert_eq!(nearest_channel(Band::Ghz2_4, 160, 1), None);
        assert!(is_valid(Band::Ghz5, 80, 42) && !is_valid(Band::Ghz5, 80, 36));
    }
}
