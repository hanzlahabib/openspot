//! Compares OpenSpot's scan of an adapter with what Windows itself reports for the same
//! networks (`netsh wlan show networks mode=bssid`).
//!
//! What this can and cannot show is spelled out in `docs/VALIDATION.md`. In short: both sides
//! read the same Windows BSS list, so agreement validates OpenSpot's decoding of channel, band
//! and PHY from the raw beacon data and the RSSI it reports, not the radio and not anything
//! derived later (heatmaps, predictions).

use std::collections::BTreeMap;

use openspot_wlan::{AccessPoint, MacAddress, PhyMode, quality_to_dbm};

use super::netsh::{NetshBss, NetshReport};

/// Signal agreement threshold, in dB. netsh reports a link-quality percentage, which Windows
/// documents as a linear map of -100..-50 dBm onto 0..100 % ([`quality_to_dbm`]); the driver's
/// percentage is an approximation of RSSI, and two scans a moment apart differ by a few dB
/// (OpenSpot's own probe merging jitters by about +-1 dB, receivers by up to +-3 dB), plus
/// 0.5 dB of rounding per percent step. 5 dB covers that without hiding a real error such as a
/// wrong sign or a 10 dB offset. It is a reporting threshold, not a claim of accuracy.
pub const SIGNAL_TOLERANCE_DB: i32 = 5;

/// The signal range the Windows quality scale covers: 0 % is -100 dBm and 100 % is -50 dBm
/// ([`quality_to_dbm`]). Anything stronger than -50 dBm still reads 100 %, so a stronger RSSI
/// is compared as -50 dBm instead of being blamed for a difference the scale cannot show.
pub const QUALITY_RANGE_DBM: (i32, i32) = (-100, -50);

/// Whether one field of one network agrees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Agreement {
    Match,
    Mismatch { native: String, netsh: String },
    /// One side did not provide the field (older netsh, or OpenSpot could not decode it).
    NotComparable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalDelta {
    pub native_dbm: i32,
    /// `native_dbm` limited to the range the quality scale can express (see [`QUALITY_RANGE_DBM`]).
    pub native_in_range_dbm: i32,
    pub netsh_percent: u8,
    /// `netsh_percent` through the documented Windows mapping.
    pub netsh_dbm: i32,
    /// `native_in_range_dbm - netsh_dbm`; positive means OpenSpot reads stronger.
    pub delta_db: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkDelta {
    pub bssid: MacAddress,
    pub channel: Agreement,
    pub band: Agreement,
    pub phy: Agreement,
    pub signal: Option<SignalDelta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comparison {
    pub matched: Vec<NetworkDelta>,
    /// Seen by OpenSpot only.
    pub only_native: Vec<MacAddress>,
    /// Listed by netsh only.
    pub only_netsh: Vec<MacAddress>,
}

pub fn compare(native: &[AccessPoint], netsh: &NetshReport) -> Comparison {
    let by_bssid: BTreeMap<MacAddress, &NetshBss> = netsh.entries.iter().map(|e| (e.bssid, e)).collect();
    let mut out = Comparison::default();
    for ap in native {
        match by_bssid.get(&ap.bssid) {
            Some(entry) => out.matched.push(delta(ap, entry)),
            None => out.only_native.push(ap.bssid),
        }
    }
    out.only_netsh = by_bssid.keys().filter(|b| !native.iter().any(|ap| ap.bssid == **b)).copied().collect();
    out
}

fn delta(ap: &AccessPoint, entry: &NetshBss) -> NetworkDelta {
    let native_band = ap.band.map(|b| b.label().to_owned());
    // The newest generation the beacon advertises is what netsh calls the radio type.
    let native_phy = ap.phy.iter().next_back().map(|p| phy_name(*p));
    NetworkDelta {
        bssid: ap.bssid,
        channel: agree(entry.channel.map(|c| c.to_string()), Some(ap.channel.to_string()).filter(|_| ap.channel != 0), |a, b| a == b),
        band: agree(entry.band.clone(), native_band, |netsh, native| normalise(netsh) == normalise(native)),
        phy: agree(entry.radio_type.clone(), native_phy, |netsh, native| normalise(netsh) == normalise(native)),
        signal: entry.signal_percent.map(|percent| {
            let netsh_dbm = quality_to_dbm(percent);
            let native_in_range_dbm = ap.rssi_dbm.clamp(QUALITY_RANGE_DBM.0, QUALITY_RANGE_DBM.1);
            SignalDelta { native_dbm: ap.rssi_dbm, native_in_range_dbm, netsh_percent: percent, netsh_dbm, delta_db: native_in_range_dbm - netsh_dbm }
        }),
    }
}

/// `802.11ax` for [`PhyMode::Ax`], the form netsh prints.
fn phy_name(phy: PhyMode) -> String {
    format!("802.11{}", phy.label())
}

/// Case- and space-insensitive form: `"5 GHz"` and `"5GHz"` are the same band.
fn normalise(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase()
}

fn agree(netsh: Option<String>, native: Option<String>, same: impl Fn(&str, &str) -> bool) -> Agreement {
    match (netsh, native) {
        (Some(netsh), Some(native)) if same(&netsh, &native) => Agreement::Match,
        (Some(netsh), Some(native)) => Agreement::Mismatch { native, netsh },
        _ => Agreement::NotComparable,
    }
}

/// Counts of agreement for one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldTally {
    pub matches: usize,
    pub mismatches: usize,
    pub not_comparable: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SignalSummary {
    pub compared: usize,
    pub within_tolerance: usize,
    pub mean_abs_db: Option<f64>,
    pub max_abs_db: Option<i32>,
    /// Mean of the signed difference: a systematic offset shows here even when the mean
    /// absolute difference is small.
    pub mean_signed_db: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Summary {
    pub native_count: usize,
    pub netsh_count: usize,
    pub matched: usize,
    pub only_native: usize,
    pub only_netsh: usize,
    pub channel: FieldTally,
    pub band: FieldTally,
    pub phy: FieldTally,
    pub signal: SignalSummary,
}

pub fn summarize(c: &Comparison) -> Summary {
    let tally = |pick: fn(&NetworkDelta) -> &Agreement| {
        c.matched.iter().fold(FieldTally::default(), |mut t, d| {
            match pick(d) {
                Agreement::Match => t.matches += 1,
                Agreement::Mismatch { .. } => t.mismatches += 1,
                Agreement::NotComparable => t.not_comparable += 1,
            }
            t
        })
    };
    let deltas: Vec<i32> = c.matched.iter().filter_map(|d| d.signal.as_ref().map(|s| s.delta_db)).collect();
    let n = deltas.len();
    let signal = SignalSummary {
        compared: n,
        within_tolerance: deltas.iter().filter(|d| d.abs() <= SIGNAL_TOLERANCE_DB).count(),
        mean_abs_db: (n > 0).then(|| deltas.iter().map(|d| f64::from(d.abs())).sum::<f64>() / n as f64),
        max_abs_db: deltas.iter().map(|d| d.abs()).max(),
        mean_signed_db: (n > 0).then(|| deltas.iter().map(|d| f64::from(*d)).sum::<f64>() / n as f64),
    };
    Summary {
        native_count: c.matched.len() + c.only_native.len(),
        netsh_count: c.matched.len() + c.only_netsh.len(),
        matched: c.matched.len(),
        only_native: c.only_native.len(),
        only_netsh: c.only_netsh.len(),
        channel: tally(|d| &d.channel),
        band: tally(|d| &d.band),
        phy: tally(|d| &d.phy),
        signal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HT: [u8; 2] = [45, 0];
    const HE: [u8; 3] = [255, 1, 35];

    fn ap(last: u8, rssi: i32, khz: u32, ies: Vec<u8>) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([2, 0, 0, 0, 0, last]), rssi, khz, true, 100, ies)
    }

    fn bss(last: u8, percent: Option<u8>, radio: Option<&str>, band: Option<&str>, channel: Option<u16>) -> NetshBss {
        NetshBss {
            ssid: String::new(),
            bssid: MacAddress([2, 0, 0, 0, 0, last]),
            signal_percent: percent,
            radio_type: radio.map(str::to_owned),
            band: band.map(str::to_owned),
            channel,
        }
    }

    fn report(entries: Vec<NetshBss>) -> NetshReport {
        NetshReport { entries, incomplete: 0 }
    }

    #[test]
    fn agreeing_networks_match_on_every_field() {
        let native = [ap(1, -55, 5_180_000, HE.to_vec()), ap(2, -80, 2_437_000, HT.to_vec())];
        let netsh = report(vec![
            bss(1, Some(90), Some("802.11ax"), Some("5 GHz"), Some(36)),
            bss(2, Some(60), Some("802.11n"), Some("2.4 GHz"), Some(6)),
        ]);
        let c = compare(&native, &netsh);
        assert_eq!(c.matched.len(), 2);
        assert!(c.only_native.is_empty() && c.only_netsh.is_empty());
        for d in &c.matched {
            assert_eq!((&d.channel, &d.band, &d.phy), (&Agreement::Match, &Agreement::Match, &Agreement::Match), "{d:?}");
        }
    }

    #[test]
    fn disagreement_names_both_values() {
        let native = [ap(1, -55, 5_180_000, HE.to_vec())];
        let netsh = report(vec![bss(1, Some(90), Some("802.11ac"), Some("2.4 GHz"), Some(40))]);
        let d = &compare(&native, &netsh).matched[0];
        assert_eq!(d.channel, Agreement::Mismatch { native: "36".into(), netsh: "40".into() });
        assert_eq!(d.band, Agreement::Mismatch { native: "5 GHz".into(), netsh: "2.4 GHz".into() });
        assert_eq!(d.phy, Agreement::Mismatch { native: "802.11ax".into(), netsh: "802.11ac".into() });
    }

    #[test]
    fn band_text_is_compared_ignoring_case_and_spacing() {
        let native = [ap(1, -55, 5_180_000, vec![])];
        let netsh = report(vec![bss(1, None, None, Some("5GHZ"), None)]);
        assert_eq!(compare(&native, &netsh).matched[0].band, Agreement::Match);
    }

    #[test]
    fn missing_fields_are_not_comparable_rather_than_mismatches() {
        let native = [ap(1, -55, 5_180_000, vec![])]; // no PHY IEs decoded
        let netsh = report(vec![bss(1, None, Some("802.11n"), None, None)]);
        let d = &compare(&native, &netsh).matched[0];
        assert_eq!((&d.channel, &d.band, &d.phy), (&Agreement::NotComparable, &Agreement::NotComparable, &Agreement::NotComparable));
        assert_eq!(d.signal, None);
    }

    #[test]
    fn networks_seen_by_one_side_only_are_listed() {
        let native = [ap(1, -55, 5_180_000, vec![]), ap(2, -60, 5_180_000, vec![])];
        let netsh = report(vec![bss(2, Some(80), None, None, None), bss(3, Some(80), None, None, None)]);
        let c = compare(&native, &netsh);
        assert_eq!(c.matched.len(), 1);
        assert_eq!(c.only_native, vec![MacAddress([2, 0, 0, 0, 0, 1])]);
        assert_eq!(c.only_netsh, vec![MacAddress([2, 0, 0, 0, 0, 3])]);
        let s = summarize(&c);
        assert_eq!((s.native_count, s.netsh_count, s.matched, s.only_native, s.only_netsh), (2, 2, 1, 1, 1));
    }

    #[test]
    fn signal_uses_the_documented_quality_mapping_and_signs_the_difference() {
        // 80 % = -60 dBm. OpenSpot reads -57: 3 dB stronger.
        let netsh = report(vec![bss(1, Some(80), None, None, None)]);
        let d = compare(&[ap(1, -57, 5_180_000, vec![])], &netsh).matched.remove(0);
        assert_eq!(d.signal, Some(SignalDelta { native_dbm: -57, native_in_range_dbm: -57, netsh_percent: 80, netsh_dbm: -60, delta_db: 3 }));
        let weaker = compare(&[ap(1, -66, 5_180_000, vec![])], &netsh).matched.remove(0);
        assert_eq!(weaker.signal.unwrap().delta_db, -6);
    }

    #[test]
    fn signals_beyond_the_quality_scale_are_compared_at_its_ends() {
        // 96 % is -52 dBm. A -37 dBm reading is far above the scale's -50 dBm end: the
        // comparison uses -50, so the difference is the scale's 2 dB, not 15 dB.
        let strong = compare(&[ap(1, -37, 5_180_000, vec![])], &report(vec![bss(1, Some(96), None, None, None)])).matched.remove(0);
        let s = strong.signal.unwrap();
        assert_eq!((s.native_dbm, s.native_in_range_dbm, s.delta_db), (-37, -50, 2));
        // And at the weak end: -105 dBm against 0 % (-100 dBm).
        let weak = compare(&[ap(1, -105, 5_180_000, vec![])], &report(vec![bss(1, Some(0), None, None, None)])).matched.remove(0);
        let weak = weak.signal.unwrap();
        assert_eq!((weak.native_in_range_dbm, weak.delta_db), (-100, 0));
        // Inside the range nothing is clamped.
        let inside = compare(&[ap(1, -70, 5_180_000, vec![])], &report(vec![bss(1, Some(65), None, None, None)])).matched.remove(0);
        assert_eq!(inside.signal.unwrap().native_in_range_dbm, -70);
    }

    #[test]
    fn summary_statistics_are_computed_exactly() {
        // Deltas: +2, -4, +9 (netsh dBm: -60, -70, -90 for 80/60/20 %).
        let native = [ap(1, -58, 5_180_000, vec![]), ap(2, -74, 5_180_000, vec![]), ap(3, -81, 5_180_000, vec![])];
        let netsh = report(vec![bss(1, Some(80), None, None, None), bss(2, Some(60), None, None, None), bss(3, Some(20), None, None, None)]);
        let s = summarize(&compare(&native, &netsh)).signal;
        assert_eq!(s.compared, 3);
        assert_eq!(s.within_tolerance, 2, "|9| exceeds the {SIGNAL_TOLERANCE_DB} dB threshold");
        assert_eq!(s.max_abs_db, Some(9));
        assert!((s.mean_abs_db.unwrap() - 5.0).abs() < 1e-9);
        assert!((s.mean_signed_db.unwrap() - 7.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_systematic_offset_is_visible_in_the_signed_mean() {
        let native = [ap(1, -50, 5_180_000, vec![]), ap(2, -60, 5_180_000, vec![])];
        let netsh = report(vec![bss(1, Some(80), None, None, None), bss(2, Some(60), None, None, None)]); // -60, -70
        let s = summarize(&compare(&native, &netsh)).signal;
        assert_eq!((s.mean_signed_db, s.mean_abs_db), (Some(10.0), Some(10.0)));
        assert_eq!(s.within_tolerance, 0);
    }

    #[test]
    fn tallies_count_each_agreement_kind() {
        let native = [ap(1, -55, 5_180_000, HE.to_vec()), ap(2, -55, 5_180_000, HE.to_vec()), ap(3, -55, 5_180_000, HE.to_vec())];
        let netsh = report(vec![
            bss(1, None, None, None, Some(36)),
            bss(2, None, None, None, Some(44)),
            bss(3, None, None, None, None),
        ]);
        let s = summarize(&compare(&native, &netsh));
        assert_eq!(s.channel, FieldTally { matches: 1, mismatches: 1, not_comparable: 1 });
        assert_eq!(s.signal, SignalSummary::default(), "no percentages, nothing compared");
    }

    #[test]
    fn empty_inputs_summarise_to_zeroes() {
        let s = summarize(&compare(&[], &NetshReport::default()));
        assert_eq!(s, Summary::default());
        assert_eq!(s.signal.mean_abs_db, None);
    }
}
