//! Hidden-SSID handling (S-04).
//!
//! A hidden AP's beacon carries a zero-length SSID (or N NUL bytes), so the BSS list
//! shows it with an empty name and [`AccessPoint::hidden`] set. The OS only reveals a
//! hidden network's name where it already knows it:
//!
//! * the BSS we are associated with (the connection attributes carry the SSID), and
//! * BSS entries whose probe response was captured (Windows then reports the name; the
//!   merge step keeps it).
//!
//! `WlanGetAvailableNetworkList` is keyed by SSID/profile, not BSSID, so it cannot name
//! an individual hidden BSSID and is deliberately not used. Anything beyond this needs a
//! user-supplied label (alias, handled in the application layer).

use crate::connection::Connection;
use crate::model::AccessPoint;

/// Fills in the SSID of hidden APs that are the BSS we are connected to. Returns how many
/// names were resolved. `hidden` stays `true`.
pub fn resolve_from_connection(aps: &mut [AccessPoint], connection: &Connection) -> usize {
    if connection.ssid.is_empty() {
        return 0;
    }
    let mut resolved = 0;
    for ap in aps
        .iter_mut()
        .filter(|a| a.ssid.is_empty() && a.bssid == connection.bssid)
    {
        ap.ssid.clone_from(&connection.ssid);
        resolved += 1;
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::ConnectionState;
    use crate::model::MacAddress;

    fn ap(last: u8, ssid: &str) -> AccessPoint {
        AccessPoint::from_beacon(
            ssid.into(),
            MacAddress([2, 0, 0, 0, 0, last]),
            -50,
            2_437_000,
            true,
            100,
            vec![],
        )
    }

    fn connection(last: u8, ssid: &str) -> Connection {
        Connection {
            state: ConnectionState::Connected,
            ssid: ssid.into(),
            bssid: MacAddress([2, 0, 0, 0, 0, last]),
            phy: None,
            rx_rate_mbps: 0.0,
            tx_rate_mbps: 0.0,
            signal_quality: 0,
            rssi_dbm: -100,
            channel: None,
        }
    }

    #[test]
    fn zero_length_and_nul_filled_ssids_are_hidden() {
        assert!(ap(1, "").hidden);
        let nul = ap(2, "\0\0\0\0");
        assert!(nul.hidden);
        assert_eq!(nul.ssid, "", "NUL-filled names are normalised to empty");
        assert!(!ap(3, "Home").hidden);
        assert!(
            !ap(4, " ").hidden,
            "a blank-space name is a real (if odd) name"
        );
    }

    #[test]
    fn only_the_connected_hidden_bss_gets_its_name() {
        let mut aps = vec![ap(1, ""), ap(2, ""), ap(3, "Named")];
        assert_eq!(
            resolve_from_connection(&mut aps, &connection(2, "Secret")),
            1
        );
        assert_eq!(aps[0].ssid, "");
        assert_eq!(aps[1].ssid, "Secret");
        assert!(aps[1].hidden);
        assert_eq!(aps[2].ssid, "Named");
    }

    #[test]
    fn named_aps_are_never_overwritten() {
        let mut aps = vec![ap(1, "Original")];
        assert_eq!(
            resolve_from_connection(&mut aps, &connection(1, "Other")),
            0
        );
        assert_eq!(aps[0].ssid, "Original");
    }
}
