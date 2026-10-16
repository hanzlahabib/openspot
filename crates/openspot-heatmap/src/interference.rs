//! Co-channel signal-to-interference ratio at one measurement point.
//!
//! The serving AP is the strongest of the wanted networks. Every other AP heard at the
//! point whose spectrum overlaps the serving channel adds interference, weighted by the
//! fraction of the serving bandwidth it covers (its power is assumed spread evenly over
//! its own width). Powers are summed in milliwatts, then expressed in dB.

use openspot_wlan::{AccessPoint, MacAddress};

/// SIR reported when nothing interferes. Real radios stop gaining above roughly this,
/// and it keeps the gradient meaningful instead of infinite.
pub const MAX_SIR_DB: f64 = 40.0;
/// SIR reported when the serving network was not heard at all (worst case).
pub const NO_SERVICE_SIR_DB: f64 = 0.0;

fn milliwatts(dbm: i32) -> f64 {
    10f64.powf(f64::from(dbm) / 10.0)
}

/// MHz of overlap between the spectrum spans of `a` and `b`.
fn overlap_mhz(a: &AccessPoint, b: &AccessPoint) -> f64 {
    if a.band != b.band {
        return 0.0;
    }
    let (ca, wa) = (a.center_frequency_mhz(), a.width_mhz());
    let (cb, wb) = (b.center_frequency_mhz(), b.width_mhz());
    let lo = (ca - wa / 2.0).max(cb - wb / 2.0);
    let hi = (ca + wa / 2.0).min(cb + wb / 2.0);
    (hi - lo).max(0.0)
}

/// SIR in dB at a point hearing `heard`, for the `wanted` networks (empty = all).
pub fn sir_db(heard: &[AccessPoint], wanted: &[MacAddress]) -> f64 {
    let serving = heard
        .iter()
        .filter(|a| wanted.is_empty() || wanted.contains(&a.bssid))
        .max_by_key(|a| a.rssi_dbm);
    let Some(serving) = serving else { return NO_SERVICE_SIR_DB };

    let interference_mw: f64 = heard
        .iter()
        .filter(|a| a.bssid != serving.bssid)
        .map(|a| milliwatts(a.rssi_dbm) * (overlap_mhz(serving, a) / serving.width_mhz()).min(1.0))
        .sum();
    if interference_mw <= 0.0 {
        return MAX_SIR_DB;
    }
    (f64::from(serving.rssi_dbm) - 10.0 * interference_mw.log10()).min(MAX_SIR_DB)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(last: u8, rssi: i32, khz: u32) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, khz, true, 100, vec![])
    }

    #[test]
    fn alone_is_capped_and_unheard_is_worst() {
        assert_eq!(sir_db(&[ap(1, -50, 2_437_000)], &[]), MAX_SIR_DB);
        assert_eq!(sir_db(&[], &[]), NO_SERVICE_SIR_DB);
        assert_eq!(sir_db(&[ap(2, -50, 2_437_000)], &[MacAddress([0, 0, 0, 0, 0, 1])]), NO_SERVICE_SIR_DB);
    }

    #[test]
    fn equal_power_co_channel_is_zero_db() {
        let sir = sir_db(&[ap(1, -50, 2_437_000), ap(2, -50, 2_437_000)], &[]);
        assert!(sir.abs() < 1e-9, "{sir}");
    }

    #[test]
    fn weaker_interferer_gives_positive_sir() {
        // 20 dB weaker interferer on the same channel -> about 20 dB SIR.
        let sir = sir_db(&[ap(1, -50, 2_437_000), ap(2, -70, 2_437_000)], &[]);
        assert!((sir - 20.0).abs() < 1e-6, "{sir}");
    }

    #[test]
    fn non_overlapping_channels_do_not_interfere() {
        // Channels 1 and 11 are 50 MHz apart.
        assert_eq!(sir_db(&[ap(1, -50, 2_412_000), ap(2, -45, 2_462_000)], &[]), MAX_SIR_DB);
    }

    #[test]
    fn partial_overlap_counts_proportionally() {
        // Channels 1 and 3 (10 MHz apart, 20 MHz wide) overlap by 10 MHz = half the width.
        let sir = sir_db(&[ap(1, -50, 2_412_000), ap(2, -50, 2_422_000)], &[]);
        assert!((sir - 10.0 * 2f64.log10()).abs() < 1e-6, "{sir}"); // 3.01 dB
    }

    #[test]
    fn different_bands_never_overlap() {
        assert_eq!(sir_db(&[ap(1, -50, 2_437_000), ap(2, -40, 5_180_000)], &[]), MAX_SIR_DB);
    }

    #[test]
    fn serving_is_the_strongest_wanted_network() {
        let heard = [ap(1, -60, 2_437_000), ap(2, -40, 2_437_000), ap(3, -55, 2_437_000)];
        // Wanted {1, 3}: serving = 3 (-55); interference = -60 and -40 summed.
        let sir = sir_db(&heard, &[MacAddress([0, 0, 0, 0, 0, 1]), MacAddress([0, 0, 0, 0, 0, 3])]);
        assert!(sir < -10.0, "{sir}");
    }
}
