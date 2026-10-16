//! Turns the adapter's current connection into what a survey point records (H-10).

use openspot_model::LinkInfo;
use openspot_wlan::{Connection, ConnectionState};

/// The link to record, or `None` when the adapter is not associated: a disconnected adapter
/// still reports stale rates on some drivers, and those must not end up on the map.
pub fn link_from_connection(connection: &Connection) -> Option<LinkInfo> {
    (connection.state == ConnectionState::Connected).then(|| LinkInfo {
        bssid: connection.bssid,
        tx_rate_mbps: sanitize(connection.tx_rate_mbps),
        rx_rate_mbps: sanitize(connection.rx_rate_mbps),
    })
}

/// Rates are never negative or infinite; anything else means "not reported" (0).
fn sanitize(mbps: f64) -> f64 {
    if mbps.is_finite() && mbps > 0.0 { mbps } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use openspot_wlan::MacAddress;

    use super::*;

    fn connection(state: ConnectionState, tx: f64, rx: f64) -> Connection {
        Connection {
            state,
            ssid: "net".into(),
            bssid: MacAddress([2, 0, 0, 0, 0, 5]),
            phy: None,
            rx_rate_mbps: rx,
            tx_rate_mbps: tx,
            signal_quality: 80,
            rssi_dbm: -60,
            channel: Some(36),
        }
    }

    #[test]
    fn a_connected_adapter_records_bssid_and_both_rates() {
        let link = link_from_connection(&connection(ConnectionState::Connected, 866.7, 780.0)).unwrap();
        assert_eq!((link.bssid, link.tx_rate_mbps, link.rx_rate_mbps), (MacAddress([2, 0, 0, 0, 0, 5]), 866.7, 780.0));
    }

    #[test]
    fn other_states_record_nothing_even_with_stale_rates() {
        for state in [ConnectionState::Disconnected, ConnectionState::Associating, ConnectionState::Authenticating, ConnectionState::NotReady] {
            assert_eq!(link_from_connection(&connection(state, 300.0, 300.0)), None, "{state:?}");
        }
    }

    #[test]
    fn unreported_or_invalid_rates_become_zero_so_they_are_skipped_by_the_map() {
        let link = link_from_connection(&connection(ConnectionState::Connected, f64::NAN, -5.0)).unwrap();
        assert_eq!((link.tx_rate_mbps, link.rx_rate_mbps), (0.0, 0.0));
    }
}
