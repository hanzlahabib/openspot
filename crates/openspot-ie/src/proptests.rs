//! Property tests (Q-02): the parser must survive any byte string, and well-formed input
//! must round-trip through the element iterator and resolve to what was encoded.

use proptest::prelude::*;

use super::*;
use crate::elements::{EXTENSION, EXT_EHT_OPERATION, VENDOR_SPECIFIC};

fn encode(id: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![id, u8::try_from(body.len()).expect("bodies are generated under 255 bytes")];
    v.extend_from_slice(body);
    v
}

fn context() -> impl Strategy<Value = Context> {
    (prop::option::of(prop_oneof![Just(Band::Ghz2_4), Just(Band::Ghz5), Just(Band::Ghz6)]), any::<bool>())
        .prop_map(|(band, privacy)| Context { band, privacy })
}

/// A well-formed element as `(id, body)`. Extension elements (id 255) need a non-empty body
/// because the first byte is the extension id.
fn element() -> impl Strategy<Value = (u8, Vec<u8>)> {
    (any::<u8>(), prop::collection::vec(any::<u8>(), 0..64)).prop_map(|(id, mut body)| {
        if id == EXTENSION && body.is_empty() {
            body.push(0);
        }
        (id, body)
    })
}

fn well_formed_run() -> impl Strategy<Value = (Vec<(u8, Vec<u8>)>, Vec<u8>)> {
    prop::collection::vec(element(), 0..12).prop_map(|els| {
        let bytes = els.iter().flat_map(|(id, body)| encode(*id, body)).collect();
        (els, bytes)
    })
}

/// RSN element body with CCMP group/pairwise and the given AKM suite types (IEEE OUI).
fn rsn_body(akms: &[u8]) -> Vec<u8> {
    let mut b = vec![1, 0, 0x00, 0x0F, 0xAC, 4, 1, 0, 0x00, 0x0F, 0xAC, 4];
    b.extend_from_slice(&u16::try_from(akms.len()).expect("few akms").to_le_bytes());
    for &a in akms {
        b.extend_from_slice(&[0x00, 0x0F, 0xAC, a]);
    }
    b.extend_from_slice(&[0, 0]);
    b
}

/// The documented AKM type -> security mapping, restated independently of the parser.
fn expected_mode(akm: u8) -> Option<SecurityMode> {
    match akm {
        1 | 3 | 5 => Some(SecurityMode::Wpa2Enterprise),
        2 | 4 | 6 => Some(SecurityMode::Wpa2Personal),
        8 | 9 => Some(SecurityMode::Wpa3Personal),
        11 | 12 => Some(SecurityMode::Wpa3Enterprise),
        18 => Some(SecurityMode::Owe),
        _ => None,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..1024), ctx in context()) {
        let parsed = parse(&bytes, ctx);
        // Security is never empty: the fallback is Open or WEP.
        prop_assert!(!parsed.security.is_empty());
    }

    #[test]
    fn every_truncation_of_a_valid_run_is_safe_and_monotone(
        (_, bytes) in well_formed_run(),
        ctx in context(),
    ) {
        let full = parse(&bytes, ctx);
        for cut in 0..=bytes.len() {
            let part = parse(&bytes[..cut], ctx);
            // PHY detection is element-presence based, so a prefix can only find fewer PHYs.
            prop_assert!(part.phy.is_subset(&full.phy), "cut {cut}: {:?} vs {:?}", part.phy, full.phy);
        }
    }

    #[test]
    fn well_formed_runs_round_trip_through_the_iterator((els, bytes) in well_formed_run()) {
        let decoded: Vec<(u8, Vec<u8>)> = Elements::new(&bytes)
            .map(|e| match e.ext_id() {
                // The iterator strips the extension id from the body; restore it to compare.
                Some(ext) => (e.id, [&[ext][..], e.body].concat()),
                None => (e.id, e.body.to_vec()),
            })
            .collect();
        prop_assert_eq!(decoded, els);
    }

    #[test]
    fn iterator_stops_cleanly_on_truncated_tail(
        (els, bytes) in well_formed_run(),
        drop_tail in 1usize..8,
    ) {
        let cut = bytes.len().saturating_sub(drop_tail);
        let n = Elements::new(&bytes[..cut]).count();
        prop_assert!(n <= els.len());
        // A prefix that ends exactly on an element boundary keeps all earlier elements.
        let boundaries: Vec<usize> = els
            .iter()
            .scan(0, |off, (_, body)| { *off += 2 + body.len(); Some(*off) })
            .collect();
        let complete = boundaries.iter().take_while(|&&b| b <= cut).count();
        prop_assert_eq!(n, complete);
    }

    #[test]
    fn capability_elements_imply_their_phy(
        (_, prefix) in well_formed_run(),
        ctx in context(),
    ) {
        for (id, ext, phy) in [
            (45u8, None, PhyMode::N),
            (191, None, PhyMode::Ac),
            (EXTENSION, Some(35u8), PhyMode::Ax),
            (EXTENSION, Some(108), PhyMode::Be),
        ] {
            let mut ies = prefix.clone();
            ies.extend(match ext {
                Some(e) => encode(id, &[e, 0, 0, 0]),
                None => encode(id, &[0; 4]),
            });
            prop_assert!(parse(&ies, ctx).phy.contains(&phy), "{phy:?} missing");
        }
    }

    #[test]
    fn rsn_akm_lists_map_to_the_documented_security_modes(
        akms in prop::collection::vec(prop::sample::select(vec![1u8, 2, 3, 4, 5, 6, 8, 9, 11, 12, 18, 7, 10, 0, 255]), 1..8),
        privacy in any::<bool>(),
    ) {
        let ies = encode(48, &rsn_body(&akms));
        let parsed = parse(&ies, Context { band: Some(Band::Ghz5), privacy });
        let expected: BTreeSet<SecurityMode> = akms.iter().filter_map(|&a| expected_mode(a)).collect();
        if expected.is_empty() {
            // Unknown AKMs only: falls back to Open/WEP by the Privacy bit.
            let fallback = if privacy { SecurityMode::Wep } else { SecurityMode::Open };
            prop_assert_eq!(parsed.security, BTreeSet::from([fallback]));
        } else {
            prop_assert_eq!(parsed.security, expected);
        }
    }

    #[test]
    fn without_security_elements_privacy_bit_decides_open_or_wep(
        // Element ids 48 (RSN) and 221 (vendor) are excluded.
        els in prop::collection::vec(
            (any::<u8>().prop_filter("not RSN/vendor", |id| *id != 48 && *id != VENDOR_SPECIFIC && *id != EXTENSION),
             prop::collection::vec(any::<u8>(), 0..32)),
            0..8),
        privacy in any::<bool>(),
    ) {
        let ies: Vec<u8> = els.iter().flat_map(|(id, b)| encode(*id, b)).collect();
        let parsed = parse(&ies, Context { band: None, privacy });
        let expected = if privacy { SecurityMode::Wep } else { SecurityMode::Open };
        prop_assert_eq!(parsed.security, BTreeSet::from([expected]));
    }

    #[test]
    fn eht_operation_wins_and_resolves_to_the_encoded_width(
        (_, prefix) in well_formed_run(),
        code in 0u8..=4,
        ccfs0 in any::<u8>(),
        ccfs1 in any::<u8>(),
        ctx in context(),
    ) {
        // EHT Operation: params (bit 0 = operation info present), basic MCS set (4 bytes),
        // then control / CCFS0 / CCFS1.
        let body = [EXT_EHT_OPERATION, 1, 0, 0, 0, 0, code, ccfs0, ccfs1];
        let mut ies = prefix;
        ies.extend(encode(EXTENSION, &body));
        let parsed = parse(&ies, ctx);
        let (want_width, want_center) = match code {
            0 => (ChannelWidth::Mhz20, ccfs0),
            1 => (ChannelWidth::Mhz40, ccfs0),
            2 => (ChannelWidth::Mhz80, ccfs0),
            3 => (ChannelWidth::Mhz160, ccfs1),
            _ => (ChannelWidth::Mhz320, ccfs1),
        };
        prop_assert_eq!(parsed.width, Some(want_width));
        prop_assert_eq!(parsed.center_channel, Some(u16::from(want_center)));
    }

    #[test]
    fn vht_operation_segments_resolve_per_the_width_rules(
        ccfs0 in 1u8..=255,
        ccfs1 in any::<u8>(),
        ctx in context(),
    ) {
        // Code 1 on VHT: ccfs1 == 0 -> 80 MHz; |ccfs0 - ccfs1| == 8 -> 160; otherwise 80+80.
        let ies = [encode(45, &[0; 4]), encode(61, &[36, 0]), encode(192, &[1, ccfs0, ccfs1, 0, 0])].concat();
        let parsed = parse(&ies, ctx);
        let want = match (ccfs1, ccfs0.abs_diff(ccfs1)) {
            (0, _) => (ChannelWidth::Mhz80, ccfs0),
            (_, 8) => (ChannelWidth::Mhz160, ccfs1),
            _ => (ChannelWidth::Mhz80Plus80, ccfs0),
        };
        prop_assert_eq!(parsed.width, Some(want.0));
        prop_assert_eq!(parsed.center_channel, Some(u16::from(want.1)));
        // The occupied bandwidth is always one of the standard widths.
        prop_assert!([20, 40, 80, 160, 320].contains(&parsed.width.map_or(0, ChannelWidth::mhz)));
    }

    #[test]
    fn plain_ht_operation_is_20_or_40_with_a_primary_channel(
        primary in any::<u8>(),
        info in any::<u8>(),
        ctx in context(),
    ) {
        let ies = [encode(45, &[0; 4]), encode(61, &[primary, info, 0, 0, 0, 0])].concat();
        let parsed = parse(&ies, ctx);
        let wide = info & 0b100 != 0 && info & 0b11 != 0;
        prop_assert_eq!(parsed.width, Some(if wide { ChannelWidth::Mhz40 } else { ChannelWidth::Mhz20 }));
        prop_assert_eq!(parsed.primary_channel, Some(u16::from(primary)));
        prop_assert_eq!(parsed.center_channel, None);
    }
}
