//! Current-connection information (S-06) and the pure conversions behind it.

use openspot_ie::PhyMode;
use serde::Serialize;

use crate::model::MacAddress;

/// Association state of an interface (`WLAN_INTERFACE_STATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConnectionState {
    NotReady,
    Connected,
    AdHocFormed,
    Disconnecting,
    Disconnected,
    Associating,
    Discovering,
    Authenticating,
    Unknown,
}

impl ConnectionState {
    /// Maps the raw `WLAN_INTERFACE_STATE` value (0..=7).
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::NotReady,
            1 => Self::Connected,
            2 => Self::AdHocFormed,
            3 => Self::Disconnecting,
            4 => Self::Disconnected,
            5 => Self::Associating,
            6 => Self::Discovering,
            7 => Self::Authenticating,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NotReady => "not ready",
            Self::Connected => "connected",
            Self::AdHocFormed => "ad hoc network formed",
            Self::Disconnecting => "disconnecting",
            Self::Disconnected => "disconnected",
            Self::Associating => "associating",
            Self::Discovering => "discovering",
            Self::Authenticating => "authenticating",
            Self::Unknown => "unknown",
        }
    }
}

/// What an interface is currently associated with.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Connection {
    pub state: ConnectionState,
    pub ssid: String,
    pub bssid: MacAddress,
    /// PHY generation the link negotiated, when the OS names one we model.
    pub phy: Option<PhyMode>,
    /// Negotiated receive rate in Mbit/s.
    pub rx_rate_mbps: f64,
    /// Negotiated transmit rate in Mbit/s.
    pub tx_rate_mbps: f64,
    /// OS signal quality, 0..=100.
    pub signal_quality: u8,
    /// Measured RSSI in dBm when the driver reports it; otherwise derived from
    /// `signal_quality` (see [`quality_to_dbm`]).
    pub rssi_dbm: i32,
    /// Primary channel number, when the driver reports it.
    pub channel: Option<u32>,
}

/// Windows defines quality 0 as -100 dBm and 100 as -50 dBm, linear in between
/// (`WLAN_SIGNAL_QUALITY` documentation).
pub fn quality_to_dbm(quality: u8) -> i32 {
    -100 + i32::from(quality.min(100)) / 2
}

/// Converts a rate in kbit/s (as the WLAN API reports) to Mbit/s.
pub fn kbps_to_mbps(kbps: u32) -> f64 {
    f64::from(kbps) / 1000.0
}

/// Maps `DOT11_PHY_TYPE` to the generations we model (`None` for legacy FHSS/IR/unknown).
pub fn phy_from_dot11(raw: i32) -> Option<PhyMode> {
    match raw {
        4 => Some(PhyMode::A),
        5 => Some(PhyMode::B),
        6 => Some(PhyMode::G),
        7 => Some(PhyMode::N),
        8 => Some(PhyMode::Ac),
        10 => Some(PhyMode::Ax),
        11 => Some(PhyMode::Be),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_maps_linearly_between_minus_100_and_minus_50() {
        assert_eq!(quality_to_dbm(0), -100);
        assert_eq!(quality_to_dbm(100), -50);
        assert_eq!(quality_to_dbm(60), -70);
        assert_eq!(quality_to_dbm(200), -50, "out-of-range input is clamped");
    }

    #[test]
    fn rates_convert_from_kbps() {
        assert!((kbps_to_mbps(866_700) - 866.7).abs() < 1e-9);
        assert_eq!(kbps_to_mbps(0), 0.0);
    }

    #[test]
    fn dot11_phy_types_map_to_generations() {
        assert_eq!(phy_from_dot11(7), Some(PhyMode::N));
        assert_eq!(phy_from_dot11(8), Some(PhyMode::Ac));
        assert_eq!(phy_from_dot11(10), Some(PhyMode::Ax));
        assert_eq!(phy_from_dot11(11), Some(PhyMode::Be));
        assert_eq!(phy_from_dot11(2), None);
        assert_eq!(phy_from_dot11(0), None);
    }

    #[test]
    fn interface_states_map_and_label() {
        assert_eq!(ConnectionState::from_raw(1), ConnectionState::Connected);
        assert_eq!(ConnectionState::from_raw(4).label(), "disconnected");
        assert_eq!(ConnectionState::from_raw(99), ConnectionState::Unknown);
    }
}
