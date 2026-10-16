//! Channel planning: which channel is least congested for a given band and width.
//!
//! Each candidate channel's congestion is the summed power (mW) of every heard AP whose
//! spectrum overlaps it, weighted by the fraction of the candidate's bandwidth covered
//! (same model as the signal-to-interference map).

use crate::model::AccessPoint;
use openspot_ie::Band;

/// Reported when nothing overlaps a candidate: the thermal-noise-ish floor of a clear channel.
pub const CLEAR_DBM: f64 = -100.0;

/// 5 GHz channels 52-144 require radar detection (DFS); some clients and APs avoid them.
const DFS_RANGE: std::ops::RangeInclusive<u16> = 52..=144;

/// One candidate channel and how busy it is.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelScore {
    /// Center channel number (for 20 MHz this is the channel itself).
    pub channel: u16,
    pub center_mhz: f64,
    pub width_mhz: u16,
    /// Summed overlapping interference power; lower is better.
    pub interference_dbm: f64,
    /// Number of heard APs overlapping this candidate.
    pub overlapping_aps: usize,
    pub dfs: bool,
}

impl ChannelScore {
    /// Plain-language congestion. -82 dBm is the usual 802.11 preamble-detect level: weaker
    /// signals do not make radios defer, so the channel is effectively free of them.
    pub fn congestion(&self) -> &'static str {
        match self.interference_dbm {
            x if x <= -82.0 => "clear",
            x if x <= -72.0 => "moderate",
            _ => "busy",
        }
    }
}

/// Valid center channels for `width_mhz` in `band` (empty if the width is not offered).
pub fn candidates(band: Band, width_mhz: u16) -> Vec<u16> {
    match (band, width_mhz) {
        // 1/6/11 are the non-overlapping set; wider channels on 2.4 GHz are discouraged.
        (Band::Ghz2_4, 20) => vec![1, 6, 11],
        (Band::Ghz5, 20) => [36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 149, 153, 157, 161, 165].into(),
        (Band::Ghz5, 40) => vec![38, 46, 54, 62, 102, 110, 118, 126, 134, 142, 151, 159],
        (Band::Ghz5, 80) => vec![42, 58, 106, 122, 138, 155],
        (Band::Ghz5, 160) => vec![50, 114],
        (Band::Ghz6, w @ (20 | 40 | 80 | 160)) => {
            let units = w / 20;
            let (first, step) = (1 + 2 * (units - 1), 4 * units);
            (0..).map(|k| first + step * k).take_while(|&c| c <= 233).collect()
        }
        _ => Vec::new(),
    }
}

fn milliwatts(dbm: i32) -> f64 {
    10f64.powf(f64::from(dbm) / 10.0)
}

/// Candidates for `band`/`width_mhz`, best (least congested) first. Ties prefer fewer
/// overlapping APs, then non-DFS, then the lower channel number.
pub fn recommend(aps: &[AccessPoint], band: Band, width_mhz: u16) -> Vec<ChannelScore> {
    let mut scores: Vec<ChannelScore> = candidates(band, width_mhz)
        .into_iter()
        .map(|channel| {
            let center_mhz = if band == Band::Ghz2_4 && channel == 14 {
                2484.0
            } else {
                f64::from(band.base_frequency_mhz()) + 5.0 * f64::from(channel)
            };
            let half = f64::from(width_mhz) / 2.0;
            let (mut power, mut overlapping) = (0.0, 0);
            for ap in aps.iter().filter(|a| a.band == Some(band)) {
                let half_ap = ap.width_mhz() / 2.0;
                let overlap = ((center_mhz + half).min(ap.center_frequency_mhz() + half_ap)
                    - (center_mhz - half).max(ap.center_frequency_mhz() - half_ap))
                .max(0.0);
                if overlap > 0.0 {
                    power += milliwatts(ap.rssi_dbm) * (overlap / f64::from(width_mhz)).min(1.0);
                    overlapping += 1;
                }
            }
            ChannelScore {
                channel,
                center_mhz,
                width_mhz,
                interference_dbm: if power > 0.0 { (10.0 * power.log10()).max(CLEAR_DBM) } else { CLEAR_DBM },
                overlapping_aps: overlapping,
                dfs: band == Band::Ghz5 && DFS_RANGE.contains(&channel),
            }
        })
        .collect();
    scores.sort_by(|a, b| {
        a.interference_dbm
            .total_cmp(&b.interference_dbm)
            .then(a.overlapping_aps.cmp(&b.overlapping_aps))
            .then(a.dfs.cmp(&b.dfs))
            .then(a.channel.cmp(&b.channel))
    });
    scores
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MacAddress;

    fn ap(last: u8, rssi: i32, khz: u32, ies: Vec<u8>) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, khz, true, 100, ies)
    }

    #[test]
    fn candidate_lists_match_the_channel_plans() {
        assert_eq!(candidates(Band::Ghz2_4, 20), vec![1, 6, 11]);
        assert!(candidates(Band::Ghz2_4, 40).is_empty());
        assert_eq!(candidates(Band::Ghz5, 80), vec![42, 58, 106, 122, 138, 155]);
        assert_eq!(candidates(Band::Ghz6, 20).len(), 59); // 1, 5, ..., 233
        assert_eq!(&candidates(Band::Ghz6, 40)[..3], &[3, 11, 19]);
        assert_eq!(&candidates(Band::Ghz6, 80)[..3], &[7, 23, 39]);
        assert_eq!(&candidates(Band::Ghz6, 160)[..3], &[15, 47, 79]);
        assert!(candidates(Band::Ghz6, 77).is_empty());
    }

    #[test]
    fn empty_air_is_clear_and_sorted_by_channel() {
        let s = recommend(&[], Band::Ghz2_4, 20);
        assert_eq!(s.iter().map(|c| c.channel).collect::<Vec<_>>(), vec![1, 6, 11]);
        assert!(s.iter().all(|c| c.interference_dbm == CLEAR_DBM && c.congestion() == "clear"));
    }

    #[test]
    fn avoids_the_occupied_channel() {
        // Strong AP on channel 6: channels 1 and 11 do not overlap it (25 MHz apart > 20).
        let s = recommend(&[ap(1, -40, 2_437_000, vec![])], Band::Ghz2_4, 20);
        assert_eq!(s.last().unwrap().channel, 6);
        assert_eq!(s.last().unwrap().overlapping_aps, 1);
        assert_eq!(s[0].channel, 1); // tie with 11 broken by lower channel
    }

    #[test]
    fn partially_overlapping_neighbor_costs_less_than_co_channel() {
        let near = recommend(&[ap(1, -50, 2_422_000, vec![])], Band::Ghz2_4, 20); // channel 3
        let on1 = near.iter().find(|c| c.channel == 1).unwrap();
        let co = recommend(&[ap(1, -50, 2_412_000, vec![])], Band::Ghz2_4, 20);
        let co1 = co.iter().find(|c| c.channel == 1).unwrap();
        assert!(on1.interference_dbm < co1.interference_dbm);
        assert!(on1.overlapping_aps == 1);
    }

    #[test]
    fn wide_neighbor_blocks_every_overlapped_20mhz_channel() {
        // 80 MHz AP centered on channel 42 occupies 36-48.
        let vht = vec![191, 0, 192, 5, 1, 42, 0, 0, 0];
        let s = recommend(&[ap(1, -45, 5_180_000, vht)], Band::Ghz5, 20);
        for ch in [36, 40, 44, 48] {
            assert!(s.iter().find(|c| c.channel == ch).unwrap().overlapping_aps == 1, "{ch}");
        }
        assert!(s[0].overlapping_aps == 0 && !s[0].dfs, "best should be a free non-DFS channel: {:?}", s[0]);
        assert_eq!(s[0].channel, 149);
    }

    #[test]
    fn other_bands_are_ignored_and_dfs_is_flagged() {
        let s = recommend(&[ap(1, -30, 2_437_000, vec![])], Band::Ghz5, 20);
        assert!(s.iter().all(|c| c.overlapping_aps == 0));
        assert!(s.iter().find(|c| c.channel == 100).unwrap().dfs);
        assert!(!s.iter().find(|c| c.channel == 36).unwrap().dfs);
        assert!(!s.iter().find(|c| c.channel == 149).unwrap().dfs);
    }

    #[test]
    fn congestion_labels() {
        let mk = |dbm| ChannelScore { channel: 1, center_mhz: 0.0, width_mhz: 20, interference_dbm: dbm, overlapping_aps: 1, dfs: false };
        assert_eq!(mk(-90.0).congestion(), "clear");
        assert_eq!(mk(-75.0).congestion(), "moderate");
        assert_eq!(mk(-50.0).congestion(), "busy");
    }
}
