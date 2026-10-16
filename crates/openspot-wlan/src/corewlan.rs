//! Pure conversions for the macOS CoreWLAN backend (S-11), tested on every platform.

use crate::error::WlanError;
use crate::model::MacAddress;

/// `CWChannelBand` raw values.
const BAND_2GHZ: i64 = 1;
const BAND_5GHZ: i64 = 2;
const BAND_6GHZ: i64 = 3;

/// Center frequency in kHz from CoreWLAN's band and channel number (IEEE 802.11 channel
/// numbering: 2.4 GHz 2407 + 5n MHz with channel 14 at 2484, 5 GHz 5000 + 5n, 6 GHz 5950 + 5n).
pub fn frequency_khz(band: i64, channel: i64) -> Option<u32> {
    let mhz = match (band, channel) {
        (BAND_2GHZ, 14) => 2484,
        (BAND_2GHZ, 1..=13) => 2407 + 5 * channel,
        (BAND_5GHZ, 1..=200) => 5000 + 5 * channel,
        (BAND_6GHZ, 1..=233) => 5950 + 5 * channel,
        _ => return None,
    };
    u32::try_from(mhz).ok().map(|m| m * 1000)
}

/// Parses CoreWLAN's `aa:bb:cc:dd:ee:ff` BSSID string. Single-digit octets are accepted
/// because CoreWLAN does not zero-pad them.
pub fn parse_bssid(text: &str) -> Option<MacAddress> {
    let mut octets = [0u8; 6];
    let mut parts = text.split(':');
    for slot in &mut octets {
        *slot = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(MacAddress(octets))
}

/// macOS 14+ redacts SSID/BSSID unless the app is authorised for Location Services; a scan
/// that returned networks but none with a BSSID therefore means missing authorisation, not
/// an empty environment.
pub fn check_not_redacted(total: usize, with_bssid: usize) -> Result<(), WlanError> {
    if total > 0 && with_bssid == 0 { Err(WlanError::AccessDenied) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_map_to_center_frequencies() {
        assert_eq!(frequency_khz(1, 1), Some(2_412_000));
        assert_eq!(frequency_khz(1, 6), Some(2_437_000));
        assert_eq!(frequency_khz(1, 14), Some(2_484_000));
        assert_eq!(frequency_khz(2, 36), Some(5_180_000));
        assert_eq!(frequency_khz(2, 149), Some(5_745_000));
        assert_eq!(frequency_khz(3, 1), Some(5_955_000));
        assert_eq!(frequency_khz(3, 5), Some(5_975_000));
    }

    #[test]
    fn invalid_band_or_channel_has_no_frequency() {
        assert_eq!(frequency_khz(0, 6), None, "unknown band");
        assert_eq!(frequency_khz(1, 0), None);
        assert_eq!(frequency_khz(1, 15), None);
        assert_eq!(frequency_khz(2, -1), None);
        assert_eq!(frequency_khz(9, 1), None);
    }

    #[test]
    fn bssid_text_parses_with_and_without_padding() {
        assert_eq!(parse_bssid("c4:47:3f:a5:c8:9c"), Some(MacAddress([0xC4, 0x47, 0x3F, 0xA5, 0xC8, 0x9C])));
        assert_eq!(parse_bssid("0:1:2:a:b:c"), Some(MacAddress([0, 1, 2, 0xA, 0xB, 0xC])));
        assert_eq!(parse_bssid("c4:47:3f:a5:c8"), None, "too short");
        assert_eq!(parse_bssid("c4:47:3f:a5:c8:9c:00"), None, "too long");
        assert_eq!(parse_bssid("zz:47:3f:a5:c8:9c"), None);
        assert_eq!(parse_bssid(""), None);
    }

    #[test]
    fn redaction_is_detected_only_when_networks_exist_without_bssids() {
        assert!(matches!(check_not_redacted(5, 0), Err(WlanError::AccessDenied)));
        assert!(check_not_redacted(0, 0).is_ok(), "an empty scan is not redaction");
        assert!(check_not_redacted(5, 1).is_ok());
    }
}
