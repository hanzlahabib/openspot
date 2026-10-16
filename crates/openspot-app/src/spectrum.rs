//! Geometry for the channel/spectrum chart.

use openspot_wlan::{AccessPoint, Band};

/// Fraction of the occupied width taken by the sloped skirt on each side; makes
/// 20 MHz signals look like the familiar OFDM "hump" instead of a box.
const SKIRT_MHZ: f64 = 3.0;

/// Closed trapezoid `[mhz, dbm]` from `floor_dbm` up to the AP's RSSI and back.
pub fn shape(ap: &AccessPoint, floor_dbm: f64) -> Vec<[f64; 2]> {
    let center = ap.center_frequency_mhz();
    let half = ap.width_mhz() / 2.0;
    let skirt = SKIRT_MHZ.min(half);
    let top = f64::from(ap.rssi_dbm).max(floor_dbm);
    vec![
        [center - half, floor_dbm],
        [center - half + skirt, top],
        [center + half - skirt, top],
        [center + half, floor_dbm],
    ]
}

/// Plot x-range (MHz) for a band, with a little margin for 2.4 GHz skirts.
pub fn band_range_mhz(band: Band) -> (f64, f64) {
    match band {
        Band::Ghz2_4 => (2397.0, 2493.0),
        Band::Ghz5 => (5150.0, 5895.0),
        Band::Ghz6 => (5925.0, 7125.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_wlan::MacAddress;

    fn wide_ap() -> AccessPoint {
        // VHT 80 MHz at center channel 42 (5210 MHz), primary 36 (5180 MHz).
        let ies = [191u8, 0, 192, 5, 1, 42, 0, 0, 0].to_vec();
        AccessPoint::from_beacon("w".into(), MacAddress([0; 6]), -50, 5_180_000, true, 100, ies)
    }

    #[test]
    fn wide_signal_is_centered_on_center_channel() {
        let ap = wide_ap();
        assert_eq!(ap.center_frequency_mhz(), 5210.0);
        assert_eq!(ap.width_mhz(), 80.0);
        let s = shape(&ap, -100.0);
        assert_eq!(s[0], [5170.0, -100.0]);
        assert_eq!(s[3], [5250.0, -100.0]);
        assert_eq!(s[1][1], -50.0);
    }

    #[test]
    fn narrow_signal_uses_beacon_frequency() {
        let ap = AccessPoint::from_beacon("n".into(), MacAddress([0; 6]), -70, 2_437_000, true, 100, vec![]);
        assert_eq!(ap.center_frequency_mhz(), 2437.0);
        let s = shape(&ap, -100.0);
        assert_eq!(s[0][0], 2427.0);
        assert_eq!(s[3][0], 2447.0);
    }

    #[test]
    fn rssi_below_floor_is_clamped() {
        let ap = AccessPoint::from_beacon("x".into(), MacAddress([0; 6]), -120, 2_437_000, true, 100, vec![]);
        assert!(shape(&ap, -100.0).iter().all(|p| p[1] >= -100.0));
    }
}
