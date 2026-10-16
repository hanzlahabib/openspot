//! nl80211 attribute decoding for the Linux backend (S-10).
//!
//! Pure byte-level code, kept free of socket I/O so it is unit-tested on every platform
//! against fixtures laid out per `include/uapi/linux/nl80211.h` and the netlink attribute
//! format (`struct nlattr`: u16 length including the 4-byte header, u16 type, payload padded
//! to 4 bytes, host byte order).

use crate::error::WlanError;
use crate::model::{AccessPoint, MacAddress};

/// `NL80211_IFTYPE_STATION`: the only interface type that can scan for networks.
pub const IFTYPE_STATION: u32 = 2;

/// `NL80211_BSS_*` values (members of the nested `NL80211_ATTR_BSS`).
const BSS_BSSID: u16 = 1;
const BSS_FREQUENCY: u16 = 2;
const BSS_BEACON_INTERVAL: u16 = 4;
const BSS_CAPABILITY: u16 = 5;
const BSS_INFORMATION_ELEMENTS: u16 = 6;
const BSS_SIGNAL_MBM: u16 = 7;
const BSS_SIGNAL_UNSPEC: u16 = 8;
const BSS_SEEN_MS_AGO: u16 = 10;
const BSS_BEACON_IES: u16 = 11;

/// `struct nlattr` header size.
const NLA_HDRLEN: usize = 4;
/// Attribute payloads are padded to this alignment.
const NLA_ALIGNTO: usize = 4;
/// Mask removing `NLA_F_NESTED` / `NLA_F_NET_BYTEORDER` from the type field.
const NLA_TYPE_MASK: u16 = 0x3FFF;
/// Capability-information Privacy bit (bit 4).
const CAPABILITY_PRIVACY: u16 = 1 << 4;
/// SSID element id and the length of an element header.
const IE_SSID: u8 = 0;
/// nl80211 reports signal in mBm (100ths of a dBm).
const MBM_PER_DBM: i32 = 100;

/// Iterates `(type, payload)` over a netlink attribute stream. Stops at the first
/// malformed attribute instead of reading out of bounds.
pub fn attrs(mut data: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    std::iter::from_fn(move || {
        let header = data.get(..NLA_HDRLEN)?;
        let len = usize::from(u16::from_ne_bytes([header[0], header[1]]));
        let ty = u16::from_ne_bytes([header[2], header[3]]) & NLA_TYPE_MASK;
        let payload = data.get(NLA_HDRLEN..len)?;
        data = data.get(len.next_multiple_of(NLA_ALIGNTO).min(data.len())..)?;
        Some((ty, payload))
    })
}

fn u16_of(b: &[u8]) -> Option<u16> {
    Some(u16::from_ne_bytes(b.get(..2)?.try_into().ok()?))
}

fn u32_of(b: &[u8]) -> Option<u32> {
    Some(u32::from_ne_bytes(b.get(..4)?.try_into().ok()?))
}

fn i32_of(b: &[u8]) -> Option<i32> {
    Some(i32::from_ne_bytes(b.get(..4)?.try_into().ok()?))
}

/// The SSID element body, if the IE run has one.
fn ssid_of(ies: &[u8]) -> Option<&[u8]> {
    openspot_ie::Elements::new(ies).find(|e| e.id == IE_SSID).map(|e| e.body)
}

/// A wireless network interface as reported by `NL80211_CMD_GET_INTERFACE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub index: u32,
    pub name: String,
}

impl Interface {
    /// Builds an interface from its attributes; `None` unless it is a station interface
    /// with an index and a name.
    pub fn from_parts(index: Option<u32>, name: Option<&[u8]>, iftype: Option<u32>) -> Option<Self> {
        if iftype != Some(IFTYPE_STATION) {
            return None;
        }
        // The name is a NUL-terminated string.
        let name = name?;
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        Some(Self { index: index?, name: String::from_utf8_lossy(&name[..end]).into_owned() })
    }
}

/// Decodes the payload of one `NL80211_ATTR_BSS` nest into an access point.
///
/// Returns `None` when the entry lacks a BSSID or frequency (nothing to key it on).
pub fn parse_bss(nest: &[u8]) -> Option<AccessPoint> {
    let (mut bssid, mut freq_mhz, mut interval, mut capability) = (None, None, 0u16, 0u16);
    let (mut ies, mut beacon_ies): (&[u8], Option<&[u8]>) = (&[], None);
    let (mut mbm, mut unspec) = (None, None);
    for (ty, payload) in attrs(nest) {
        match ty {
            BSS_BSSID => bssid = <[u8; 6]>::try_from(payload).ok().map(MacAddress),
            BSS_FREQUENCY => freq_mhz = u32_of(payload),
            BSS_BEACON_INTERVAL => interval = u16_of(payload).unwrap_or(0),
            BSS_CAPABILITY => capability = u16_of(payload).unwrap_or(0),
            BSS_INFORMATION_ELEMENTS => ies = payload,
            BSS_BEACON_IES => beacon_ies = Some(payload),
            BSS_SIGNAL_MBM => mbm = i32_of(payload),
            BSS_SIGNAL_UNSPEC => unspec = payload.first().copied(),
            _ => {}
        }
    }
    let rssi = match (mbm, unspec) {
        (Some(mbm), _) => (f64::from(mbm) / f64::from(MBM_PER_DBM)).round() as i32,
        (None, Some(quality)) => crate::connection::quality_to_dbm(quality),
        // No signal information at all: report the weakest value rather than invent one.
        (None, None) => i32::MIN / 2,
    };
    // `ies` come from the last beacon *or probe response*; a hidden AP's beacon has an empty
    // SSID while its probe response reveals the name. Hidden status follows the beacon.
    let beacon_view = beacon_ies.filter(|b| !b.is_empty()).unwrap_or(ies);
    let beacon_ssid = String::from_utf8_lossy(ssid_of(beacon_view).unwrap_or_default()).into_owned();
    let mut ap = AccessPoint::from_beacon(
        beacon_ssid,
        bssid?,
        rssi,
        freq_mhz?.checked_mul(1000)?,
        capability & CAPABILITY_PRIVACY != 0,
        interval,
        ies.to_vec(),
    );
    if ap.hidden {
        if let Some(name) = ssid_of(ies).filter(|n| n.iter().any(|&b| b != 0)) {
            ap.ssid = String::from_utf8_lossy(name).into_owned();
        }
    }
    Some(ap)
}

/// How long ago the kernel last saw this BSS, in milliseconds.
pub fn seen_ms_ago(nest: &[u8]) -> Option<u32> {
    attrs(nest).find(|(ty, _)| *ty == BSS_SEEN_MS_AGO).and_then(|(_, p)| u32_of(p))
}

const EPERM: i32 = 1;
const ENOENT: i32 = 2;
const EACCES: i32 = 13;
const ENODEV: i32 = 19;
const ERFKILL: i32 = 132;

/// Maps a (positive) errno from a netlink error message to a typed error. `None` means the
/// code is not one we translate (callers decide whether it is fatal).
pub fn error_for_errno(errno: i32) -> Option<WlanError> {
    match errno {
        EPERM | EACCES => Some(WlanError::PermissionDenied),
        ERFKILL => Some(WlanError::RadioOff),
        ENODEV | ENOENT => Some(WlanError::NoAdapters),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_ie::{Band, PhyMode, SecurityMode};

    /// Encodes one netlink attribute.
    fn nla(ty: u16, payload: &[u8]) -> Vec<u8> {
        let len = u16::try_from(NLA_HDRLEN + payload.len()).unwrap();
        let mut v = [len.to_ne_bytes(), ty.to_ne_bytes()].concat();
        v.extend_from_slice(payload);
        v.resize(v.len().next_multiple_of(NLA_ALIGNTO), 0);
        v
    }

    fn ie(id: u8, body: &[u8]) -> Vec<u8> {
        [&[id, body.len() as u8][..], body].concat()
    }

    fn bss(ssid: &str, mbm: i32, freq: u32, extra: &[Vec<u8>]) -> Vec<u8> {
        let ies = [ie(0, ssid.as_bytes()), ie(3, &[6]), ie(45, &[0; 26]), ie(61, &[6, 0, 0, 0, 0, 0])].concat();
        [
            nla(BSS_BSSID, &[0x02, 0xAA, 0xBB, 0xCC, 0xDD, 0x01]),
            nla(BSS_FREQUENCY, &freq.to_ne_bytes()),
            nla(BSS_BEACON_INTERVAL, &100u16.to_ne_bytes()),
            nla(BSS_CAPABILITY, &0x0411u16.to_ne_bytes()), // ESS | Privacy | short slot
            nla(BSS_SIGNAL_MBM, &mbm.to_ne_bytes()),
            nla(BSS_INFORMATION_ELEMENTS, &ies),
            extra.concat(),
        ]
        .concat()
    }

    #[test]
    fn attribute_stream_handles_padding_and_nesting_flag() {
        let data = [nla(1, &[1, 2, 3]), nla(2 | 0x8000, &[9, 9, 9, 9]), nla(3, &[])].concat();
        let got: Vec<_> = attrs(&data).collect();
        assert_eq!(got, vec![(1, &[1u8, 2, 3][..]), (2, &[9u8, 9, 9, 9][..]), (3, &[][..])]);
    }

    #[test]
    fn attribute_stream_stops_on_truncation_without_panicking() {
        let data = nla(1, &[1, 2, 3, 4, 5, 6]);
        // 4-byte header + 6-byte payload = 10 bytes; the 2 padding bytes are optional.
        for cut in 0..data.len() {
            assert_eq!(attrs(&data[..cut]).count(), usize::from(cut >= 10), "cut {cut}");
        }
        // A length shorter than the header is malformed.
        assert_eq!(attrs(&[2, 0, 1, 0]).count(), 0);
    }

    #[test]
    fn bss_fields_are_decoded() {
        let ap = parse_bss(&bss("Lab", -5230, 2437, &[])).unwrap();
        assert_eq!(ap.ssid, "Lab");
        assert!(!ap.hidden);
        assert_eq!(ap.bssid, MacAddress([0x02, 0xAA, 0xBB, 0xCC, 0xDD, 0x01]));
        assert_eq!(ap.rssi_dbm, -52, "-52.30 dBm rounds to -52");
        assert_eq!(ap.frequency_khz, 2_437_000);
        assert_eq!(ap.channel, 6);
        assert_eq!(ap.band, Some(Band::Ghz2_4));
        assert_eq!(ap.beacon_interval_tu, 100);
        assert!(ap.phy.contains(&PhyMode::N));
        assert!(ap.privacy(), "Privacy capability bit set and no RSN -> WEP");
        assert_eq!(ap.security, [SecurityMode::Wep].into());
        assert!(!ap.raw_ies.is_empty());
    }

    #[test]
    fn signal_falls_back_to_unspec_quality_and_rounds_mbm() {
        let nest = [
            nla(BSS_BSSID, &[2, 0, 0, 0, 0, 9]),
            nla(BSS_FREQUENCY, &5180u32.to_ne_bytes()),
            nla(BSS_SIGNAL_UNSPEC, &[60]),
        ]
        .concat();
        assert_eq!(parse_bss(&nest).unwrap().rssi_dbm, -70, "quality 60 -> -70 dBm");
        assert_eq!(parse_bss(&bss("x", -6750, 5180, &[])).unwrap().rssi_dbm, -68, "-67.50 rounds away from zero");
    }

    #[test]
    fn hidden_beacon_with_named_probe_response_keeps_hidden_flag_and_name() {
        let beacon_ies = [ie(0, &[]), ie(3, &[1])].concat();
        let ap = parse_bss(&bss("Named", -4000, 2412, &[nla(BSS_BEACON_IES, &beacon_ies)])).unwrap();
        assert!(ap.hidden, "the beacon carried no SSID");
        assert_eq!(ap.ssid, "Named", "the probe response revealed it");
    }

    #[test]
    fn nul_filled_ssid_is_hidden_and_unnamed() {
        let ap = parse_bss(&bss("\0\0\0\0", -4000, 2412, &[])).unwrap();
        assert!(ap.hidden);
        assert_eq!(ap.ssid, "");
    }

    #[test]
    fn entries_missing_bssid_or_frequency_are_dropped() {
        let no_freq = nla(BSS_BSSID, &[2, 0, 0, 0, 0, 1]);
        let no_bssid = nla(BSS_FREQUENCY, &2412u32.to_ne_bytes());
        let short_bssid = [nla(BSS_BSSID, &[1, 2, 3]), nla(BSS_FREQUENCY, &2412u32.to_ne_bytes())].concat();
        assert!(parse_bss(&no_freq).is_none());
        assert!(parse_bss(&no_bssid).is_none());
        assert!(parse_bss(&short_bssid).is_none());
        assert!(parse_bss(&[]).is_none());
    }

    #[test]
    fn six_ghz_frequency_maps_to_band_and_channel() {
        let ap = parse_bss(&bss("x", -5000, 5975, &[])).unwrap();
        assert_eq!(ap.band, Some(Band::Ghz6));
        assert_eq!(ap.channel, 5);
    }

    #[test]
    fn only_named_station_interfaces_are_adapters() {
        let station = Interface::from_parts(Some(3), Some(b"wlan0\0"), Some(IFTYPE_STATION)).unwrap();
        assert_eq!(station, Interface { index: 3, name: "wlan0".into() });
        assert!(Interface::from_parts(Some(4), Some(b"mon0\0"), Some(6)).is_none(), "monitor mode");
        assert!(Interface::from_parts(None, Some(b"wlan1\0"), Some(IFTYPE_STATION)).is_none());
        assert!(Interface::from_parts(Some(5), None, Some(IFTYPE_STATION)).is_none());
        assert!(Interface::from_parts(Some(5), Some(b"p2p"), None).is_none());
    }

    #[test]
    fn seen_ms_ago_is_read_when_present() {
        let nest = [nla(BSS_SEEN_MS_AGO, &1234u32.to_ne_bytes()), nla(BSS_BSSID, &[0; 6])].concat();
        assert_eq!(seen_ms_ago(&nest), Some(1234));
        assert_eq!(seen_ms_ago(&nla(BSS_BSSID, &[0; 6])), None);
    }

    #[test]
    fn errno_mapping() {
        assert!(matches!(error_for_errno(1), Some(WlanError::PermissionDenied)));
        assert!(matches!(error_for_errno(13), Some(WlanError::PermissionDenied)));
        assert!(matches!(error_for_errno(132), Some(WlanError::RadioOff)));
        assert!(matches!(error_for_errno(19), Some(WlanError::NoAdapters)));
        assert!(error_for_errno(16).is_none(), "EBUSY is handled by the caller");
    }
}
