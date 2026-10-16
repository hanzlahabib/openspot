//! RSN (WPA2/WPA3/OWE) and WPA1 vendor element parsing.

use std::collections::BTreeSet;

use crate::SecurityMode;
use crate::elements::{Elements, RSN, VENDOR_SPECIFIC};

/// IEEE 802.11 OUI used by RSN cipher and AKM suites.
const OUI_IEEE: [u8; 3] = [0x00, 0x0F, 0xAC];
/// Microsoft OUI + type 1 identifies the pre-standard WPA element.
const WPA1_PREFIX: [u8; 4] = [0x00, 0x50, 0xF2, 0x01];
const WPA1_AKM_OUI: [u8; 3] = [0x00, 0x50, 0xF2];

pub(crate) fn parse(ies: &[u8], privacy: bool) -> BTreeSet<SecurityMode> {
    let mut set = BTreeSet::new();
    for el in Elements::new(ies) {
        match el.id {
            RSN => set.extend(rsn_akms(el.body).filter_map(rsn_mode)),
            VENDOR_SPECIFIC if el.body.starts_with(&WPA1_PREFIX) => {
                set.extend(wpa1_akms(&el.body[WPA1_PREFIX.len()..]).filter_map(wpa1_mode));
            }
            _ => {}
        }
    }
    if set.is_empty() {
        set.insert(if privacy { SecurityMode::Wep } else { SecurityMode::Open });
    }
    set
}

/// Skips version, group cipher and pairwise list, then yields AKM suite types
/// whose OUI is the IEEE one. Layout shared by RSN and WPA1 (after its prefix).
fn akm_list(body: &[u8], akm_oui: [u8; 3]) -> impl Iterator<Item = u8> + '_ {
    let types = (|| {
        let after_version = body.get(2..)?; // RSN version / WPA version
        let after_group = after_version.get(4..)?;
        let pairwise = usize::from(u16::from_le_bytes([*after_group.first()?, *after_group.get(1)?]));
        let after_pairwise = after_group.get(2 + pairwise * 4..)?;
        let akm_count = usize::from(u16::from_le_bytes([*after_pairwise.first()?, *after_pairwise.get(1)?]));
        after_pairwise.get(2..2 + akm_count * 4)
    })()
    .unwrap_or(&[]);
    types
        .chunks_exact(4)
        .filter(move |s| s[..3] == akm_oui)
        .map(|s| s[3])
}

fn rsn_akms(body: &[u8]) -> impl Iterator<Item = u8> + '_ {
    akm_list(body, OUI_IEEE)
}

fn wpa1_akms(body: &[u8]) -> impl Iterator<Item = u8> + '_ {
    akm_list(body, WPA1_AKM_OUI)
}

fn rsn_mode(akm: u8) -> Option<SecurityMode> {
    Some(match akm {
        1 | 3 | 5 => SecurityMode::Wpa2Enterprise,
        2 | 4 | 6 => SecurityMode::Wpa2Personal,
        8 | 9 => SecurityMode::Wpa3Personal,
        11 | 12 => SecurityMode::Wpa3Enterprise,
        18 => SecurityMode::Owe,
        _ => return None,
    })
}

fn wpa1_mode(akm: u8) -> Option<SecurityMode> {
    match akm {
        1 => Some(SecurityMode::WpaEnterprise),
        2 => Some(SecurityMode::WpaPersonal),
        _ => None,
    }
}
