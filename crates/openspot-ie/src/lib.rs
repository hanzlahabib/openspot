//! Parser for 802.11 information elements (IEs) found in beacons and probe
//! responses. Written from IEEE 802.11-2020 / 802.11ax / 802.11be element layouts.
//!
//! Input is the raw IE byte run (a sequence of `id, len, body` records); output is
//! the PHY generations, security modes and channel width the AP advertises.
//! Truncated or malformed elements are skipped, never panicked on.

#![forbid(unsafe_code)]

mod elements;
mod security;
mod width;

use std::collections::BTreeSet;

use serde::Serialize;

pub use elements::{Element, Elements};

/// Radio band, derived from the channel center frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Band {
    #[serde(rename = "2.4 GHz")]
    Ghz2_4,
    #[serde(rename = "5 GHz")]
    Ghz5,
    #[serde(rename = "6 GHz")]
    Ghz6,
}

impl Band {
    /// Human-readable name, e.g. "2.4 GHz".
    pub fn label(self) -> &'static str {
        match self {
            Self::Ghz2_4 => "2.4 GHz",
            Self::Ghz5 => "5 GHz",
            Self::Ghz6 => "6 GHz",
        }
    }

    /// Frequency in MHz of channel 0 for this band; channel `n` is at `base + 5 * n`
    /// (channel 14 on 2.4 GHz is the one exception).
    pub fn base_frequency_mhz(self) -> u32 {
        match self {
            Self::Ghz2_4 => 2407,
            Self::Ghz5 => 5000,
            Self::Ghz6 => 5950,
        }
    }

    /// Classifies a center frequency in kHz (as reported by the OS WLAN APIs).
    pub fn from_frequency_khz(khz: u32) -> Option<Self> {
        match khz {
            2_400_000..=2_500_000 => Some(Self::Ghz2_4),
            4_900_000..=5_900_000 => Some(Self::Ghz5),
            5_925_000..=7_125_000 => Some(Self::Ghz6),
            _ => None,
        }
    }

    /// IEEE channel number for a center frequency in this band.
    pub fn channel_for_frequency_khz(self, khz: u32) -> u16 {
        let mhz = khz / 1000;
        let ch = match self {
            Self::Ghz2_4 if mhz == 2484 => 14,
            Self::Ghz2_4 => (mhz.saturating_sub(2407)) / 5,
            Self::Ghz5 => (mhz.saturating_sub(5000)) / 5,
            Self::Ghz6 => (mhz.saturating_sub(5950)) / 5,
        };
        ch as u16
    }
}

/// Wi-Fi PHY generation (amendment) advertised by an AP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum PhyMode {
    /// 802.11b (DSSS/CCK).
    B,
    /// 802.11a (OFDM, 5 GHz).
    A,
    /// 802.11g (ERP-OFDM, 2.4 GHz).
    G,
    /// 802.11n, Wi-Fi 4.
    N,
    /// 802.11ac, Wi-Fi 5.
    Ac,
    /// 802.11ax, Wi-Fi 6/6E.
    Ax,
    /// 802.11be, Wi-Fi 7.
    Be,
}

impl PhyMode {
    /// IEEE amendment letter(s), e.g. "ax".
    pub fn label(self) -> &'static str {
        match self {
            Self::B => "b",
            Self::A => "a",
            Self::G => "g",
            Self::N => "n",
            Self::Ac => "ac",
            Self::Ax => "ax",
            Self::Be => "be",
        }
    }
}

/// Authentication/encryption family. A mixed-mode AP reports several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum SecurityMode {
    Open,
    Wep,
    WpaPersonal,
    WpaEnterprise,
    Wpa2Personal,
    Wpa2Enterprise,
    Wpa3Personal,
    Wpa3Enterprise,
    /// Opportunistic Wireless Encryption ("Enhanced Open").
    Owe,
}

impl SecurityMode {
    /// Marketing-style name, e.g. "WPA2-Personal".
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Wep => "WEP",
            Self::WpaPersonal => "WPA-Personal",
            Self::WpaEnterprise => "WPA-Enterprise",
            Self::Wpa2Personal => "WPA2-Personal",
            Self::Wpa2Enterprise => "WPA2-Enterprise",
            Self::Wpa3Personal => "WPA3-Personal",
            Self::Wpa3Enterprise => "WPA3-Enterprise",
            Self::Owe => "OWE",
        }
    }
}

/// Operating channel width in MHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ChannelWidth {
    Mhz20,
    Mhz40,
    Mhz80,
    Mhz160,
    /// 80+80 non-contiguous.
    Mhz80Plus80,
    Mhz320,
}

impl ChannelWidth {
    /// Occupied bandwidth in MHz (80+80 counts both segments).
    pub fn mhz(self) -> u16 {
        match self {
            Self::Mhz20 => 20,
            Self::Mhz40 => 40,
            Self::Mhz80 => 80,
            Self::Mhz160 | Self::Mhz80Plus80 => 160,
            Self::Mhz320 => 320,
        }
    }
}

/// Facts from the frame that are not carried inside the IEs.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// Band the frame was received on; needed to tell 802.11a from b/g.
    pub band: Option<Band>,
    /// Capability-information "Privacy" bit; distinguishes Open from WEP.
    pub privacy: bool,
}

/// Everything extracted from one AP's IEs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Parsed {
    pub phy: BTreeSet<PhyMode>,
    pub security: BTreeSet<SecurityMode>,
    pub width: Option<ChannelWidth>,
    /// Center channel index of the widest operating segment, if advertised.
    pub center_channel: Option<u16>,
    /// Primary channel from DS Parameter Set / HT / HE 6 GHz operation.
    pub primary_channel: Option<u16>,
}

/// Parses a raw IE run.
pub fn parse(ies: &[u8], ctx: Context) -> Parsed {
    let mut out = Parsed::default();
    let mut rates: Vec<u8> = Vec::new();
    let mut width = width::Width::default();

    for el in Elements::new(ies) {
        match (el.id, el.ext_id()) {
            (elements::DS_PARAMETER, _) => {
                out.primary_channel = el.body.first().map(|&c| u16::from(c));
            }
            (elements::SUPPORTED_RATES | elements::EXTENDED_RATES, _) => {
                rates.extend(el.body.iter().map(|b| b & 0x7F));
            }
            (elements::HT_CAPABILITIES, _) => {
                out.phy.insert(PhyMode::N);
            }
            (elements::HT_OPERATION, _) => width.ht(el.body),
            (elements::VHT_CAPABILITIES, _) => {
                out.phy.insert(PhyMode::Ac);
            }
            (elements::VHT_OPERATION, _) => width.vht(el.body),
            (elements::EXTENSION, Some(elements::EXT_HE_CAPABILITIES)) => {
                out.phy.insert(PhyMode::Ax);
            }
            (elements::EXTENSION, Some(elements::EXT_HE_OPERATION)) => width.he(el.body),
            (elements::EXTENSION, Some(elements::EXT_EHT_CAPABILITIES)) => {
                out.phy.insert(PhyMode::Be);
            }
            (elements::EXTENSION, Some(elements::EXT_EHT_OPERATION)) => width.eht(el.body),
            _ => {}
        }
    }

    out.phy.extend(legacy_phy(&rates, ctx.band));
    out.security = security::parse(ies, ctx.privacy);
    let resolved = width.resolve();
    out.width = resolved.width.or(Some(ChannelWidth::Mhz20).filter(|_| !out.phy.is_empty()));
    out.center_channel = resolved.center_channel;
    out.primary_channel = width.primary_channel().or(out.primary_channel);
    out
}

/// Legacy PHYs inferred from the basic/extended supported-rates sets.
///
/// Rates are in 500 kbit/s units: 1/2/5.5/11 Mbit/s are DSSS/CCK (b); the
/// 6..54 Mbit/s set is OFDM (a on 5/6 GHz, g on 2.4 GHz).
fn legacy_phy(rates: &[u8], band: Option<Band>) -> BTreeSet<PhyMode> {
    const CCK: [u8; 4] = [2, 4, 11, 22];
    const OFDM: [u8; 8] = [12, 18, 24, 36, 48, 72, 96, 108];

    let mut set = BTreeSet::new();
    if rates.iter().any(|r| CCK.contains(r)) {
        set.insert(PhyMode::B);
    }
    if rates.iter().any(|r| OFDM.contains(r)) {
        match band {
            Some(Band::Ghz2_4) => set.insert(PhyMode::G),
            // 5/6 GHz, or unknown band with OFDM-only rates, is 802.11a.
            _ => set.insert(PhyMode::A),
        };
    }
    set
}

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod tests;
