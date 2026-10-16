use std::collections::BTreeSet;
use std::fmt;

use openspot_ie::{Band, ChannelWidth, PhyMode, SecurityMode};
use serde::{Serialize, Serializer};

/// 48-bit hardware address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddress(pub [u8; 6]);

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02X}:{b:02X}:{c:02X}:{d:02X}:{e:02X}:{g:02X}")
    }
}

impl Serialize for MacAddress {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// A wireless network interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Adapter {
    /// Stable OS identifier (interface GUID on Windows).
    pub id: String,
    pub description: String,
}

/// BSS topology: an access point (ESS) or a peer-to-peer ad-hoc network (IBSS).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum NetworkType {
    #[default]
    Infrastructure,
    AdHoc,
}

impl NetworkType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Infrastructure => "Infrastructure",
            Self::AdHoc => "Ad-hoc",
        }
    }
}

/// One BSS (access point radio) seen in a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccessPoint {
    /// Empty string for hidden networks, unless the OS reported the name (see [`crate::hidden`]).
    pub ssid: String,
    /// The beacon advertised no usable SSID (zero-length or NUL-filled). Stays `true`
    /// when the name was later resolved, so the UI can still flag the network as hidden.
    pub hidden: bool,
    pub bssid: MacAddress,
    pub rssi_dbm: i32,
    pub frequency_khz: u32,
    pub channel: u16,
    pub band: Option<Band>,
    pub phy: BTreeSet<PhyMode>,
    pub security: BTreeSet<SecurityMode>,
    pub width: Option<ChannelWidth>,
    pub center_channel: Option<u16>,
    pub beacon_interval_tu: u16,
    /// Infrastructure unless the OS reported an independent (ad-hoc) BSS.
    pub network_type: NetworkType,
    /// Raw IE bytes, kept so parsing can be improved without re-surveying.
    #[serde(serialize_with = "hex")]
    pub raw_ies: Vec<u8>,
}

fn hex<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
    use fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Writing to a String cannot fail.
        let _ = write!(out, "{b:02x}");
    }
    s.serialize_str(&out)
}

/// Hidden networks broadcast either a zero-length SSID or one of N NUL bytes (the 802.11
/// "hidden" conventions); both carry no name.
fn is_hidden_ssid(ssid: &str) -> bool {
    ssid.chars().all(|c| c == '\0')
}

impl AccessPoint {
    /// Center of the occupied spectrum in MHz. Wide (40+ MHz) signals use the advertised
    /// center channel; narrow ones use the beacon's own frequency.
    pub fn center_frequency_mhz(&self) -> f64 {
        match (self.band, self.center_channel) {
            (Some(band), Some(center)) if self.width.is_some_and(|w| w.mhz() > 20) => {
                f64::from(band.base_frequency_mhz()) + 5.0 * f64::from(center)
            }
            _ => f64::from(self.frequency_khz) / 1000.0,
        }
    }

    /// Occupied bandwidth in MHz (20 when unknown).
    pub fn width_mhz(&self) -> f64 {
        f64::from(self.width.map_or(20, |w| w.mhz()))
    }

    /// Whether the beacon's Privacy capability bit was set (anything but an open network).
    pub fn privacy(&self) -> bool {
        self.security != BTreeSet::from([SecurityMode::Open])
    }

    /// Builds an AP from the OS-reported fields, deriving everything else from the IEs.
    pub fn from_beacon(
        ssid: String,
        bssid: MacAddress,
        rssi_dbm: i32,
        frequency_khz: u32,
        privacy: bool,
        beacon_interval_tu: u16,
        raw_ies: Vec<u8>,
    ) -> Self {
        let hidden = is_hidden_ssid(&ssid);
        let ssid = if hidden { String::new() } else { ssid };
        let band = Band::from_frequency_khz(frequency_khz);
        let parsed = openspot_ie::parse(&raw_ies, openspot_ie::Context { band, privacy });
        let channel = band
            .map(|b| b.channel_for_frequency_khz(frequency_khz))
            .or(parsed.primary_channel)
            .unwrap_or(0);
        Self {
            ssid,
            hidden,
            bssid,
            rssi_dbm,
            frequency_khz,
            channel,
            band,
            phy: parsed.phy,
            security: parsed.security,
            width: parsed.width,
            center_channel: parsed.center_channel,
            beacon_interval_tu,
            network_type: NetworkType::Infrastructure,
            raw_ies,
        }
    }

    /// Overrides the BSS type for backends that report it (defaults to infrastructure).
    #[must_use]
    pub fn with_network_type(mut self, network_type: NetworkType) -> Self {
        self.network_type = network_type;
        self
    }
}
