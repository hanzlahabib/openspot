//! The adapter's own wireless link at the moment a point was measured (H-10).

use openspot_wlan::MacAddress;

/// What the adapter reported about the network it is connected to while a point was recorded.
///
/// This is the link the *laptop* has, not a property of every access point it can hear: a
/// transmit-rate map built from it shows the quality of the connection to whichever AP the
/// laptop was associated with at each spot.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkInfo {
    /// BSSID of the access point the adapter was associated with.
    pub bssid: MacAddress,
    /// Negotiated transmit rate in Mbit/s; 0 when the driver reported none.
    pub tx_rate_mbps: f64,
    /// Negotiated receive rate in Mbit/s; 0 when the driver reported none.
    pub rx_rate_mbps: f64,
}
