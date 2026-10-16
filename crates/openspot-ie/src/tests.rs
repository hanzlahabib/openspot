use super::*;

/// Builds `id | len | body` records.
fn el(id: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![id, body.len() as u8];
    v.extend_from_slice(body);
    v
}

fn ext(ext_id: u8, body: &[u8]) -> Vec<u8> {
    let mut b = vec![ext_id];
    b.extend_from_slice(body);
    el(255, &b)
}

fn ctx(band: Band, privacy: bool) -> Context {
    Context { band: Some(band), privacy }
}

fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

/// RSN element body: version 1, CCMP group/pairwise, given AKM type bytes.
fn rsn(akms: &[u8]) -> Vec<u8> {
    let mut b = vec![1, 0, 0x00, 0x0F, 0xAC, 4, 1, 0, 0x00, 0x0F, 0xAC, 4];
    b.extend_from_slice(&(akms.len() as u16).to_le_bytes());
    for &a in akms {
        b.extend_from_slice(&[0x00, 0x0F, 0xAC, a]);
    }
    b.extend_from_slice(&[0, 0]); // RSN capabilities
    el(48, &b)
}

#[test]
fn band_and_channel_from_frequency() {
    let b = Band::from_frequency_khz(2_412_000).unwrap();
    assert_eq!(b, Band::Ghz2_4);
    assert_eq!(b.channel_for_frequency_khz(2_412_000), 1);
    assert_eq!(Band::Ghz2_4.channel_for_frequency_khz(2_484_000), 14);
    assert_eq!(Band::Ghz5.channel_for_frequency_khz(5_180_000), 36);
    assert_eq!(Band::from_frequency_khz(5_955_000), Some(Band::Ghz6));
    assert_eq!(Band::Ghz6.channel_for_frequency_khz(5_955_000), 1);
    assert_eq!(Band::from_frequency_khz(900_000), None);
}

#[test]
fn legacy_rates_2g_give_b_and_g() {
    // 1, 2, 5.5, 11 (basic) and 6..54 Mbit/s.
    let ies = cat(&[
        el(1, &[0x82, 0x84, 0x8B, 0x96, 0x0C, 0x12, 0x18, 0x24]),
        el(50, &[0x30, 0x48, 0x60, 0x6C]),
    ]);
    let p = parse(&ies, ctx(Band::Ghz2_4, false));
    assert_eq!(p.phy, BTreeSet::from([PhyMode::B, PhyMode::G]));
    assert_eq!(p.width, Some(ChannelWidth::Mhz20));
}

#[test]
fn ofdm_rates_5g_give_a() {
    let ies = el(1, &[0x8C, 0x12, 0x98, 0x24, 0xB0, 0x48, 0x60, 0x6C]);
    let p = parse(&ies, ctx(Band::Ghz5, false));
    assert_eq!(p.phy, BTreeSet::from([PhyMode::A]));
}

#[test]
fn wifi5_80mhz_with_center_channel() {
    let ies = cat(&[
        el(1, &[0x8C, 0x12, 0x98, 0x24]),
        el(45, &[0; 26]),
        el(61, &[36, 0b0000_0101, 0, 0, 0, 0]), // primary 36, 40 MHz allowed, offset above
        el(191, &[0; 12]),
        el(192, &[1, 42, 0, 0, 0]), // 80 MHz, center channel 42
    ]);
    let p = parse(&ies, ctx(Band::Ghz5, false));
    assert_eq!(p.phy, BTreeSet::from([PhyMode::A, PhyMode::N, PhyMode::Ac]));
    assert_eq!(p.width, Some(ChannelWidth::Mhz80));
    assert_eq!(p.center_channel, Some(42));
    assert_eq!(p.primary_channel, Some(36));
}

#[test]
fn vht_160_signalled_by_ccfs1() {
    let ies = cat(&[el(191, &[0; 12]), el(192, &[1, 42, 50, 0, 0])]);
    let p = parse(&ies, ctx(Band::Ghz5, false));
    assert_eq!(p.width, Some(ChannelWidth::Mhz160));
    assert_eq!(p.center_channel, Some(50));
}

#[test]
fn vht_80_plus_80() {
    let ies = el(192, &[1, 42, 106, 0, 0]);
    let p = parse(&ies, ctx(Band::Ghz5, false));
    assert_eq!(p.width, Some(ChannelWidth::Mhz80Plus80));
}

#[test]
fn ht_40mhz_on_2g() {
    let ies = cat(&[el(45, &[0; 26]), el(61, &[6, 0b0000_0101, 0, 0, 0, 0]), el(3, &[6])]);
    let p = parse(&ies, ctx(Band::Ghz2_4, false));
    assert_eq!(p.width, Some(ChannelWidth::Mhz40));
    assert_eq!(p.primary_channel, Some(6));
    assert!(p.phy.contains(&PhyMode::N));
}

#[test]
fn he_on_6ghz_reports_width_and_primary() {
    // Parameters: only the 6 GHz information bit (17) set.
    let params = (1u32 << 17).to_le_bytes();
    let mut body = vec![params[0], params[1], params[2], 0, 0, 0];
    body.extend_from_slice(&[5, 0b11, 15, 23, 0]); // primary 5, 160/80+80, ccfs0 15, ccfs1 23
    let ies = cat(&[ext(35, &[0; 20]), ext(36, &body)]);
    let p = parse(&ies, ctx(Band::Ghz6, false));
    assert!(p.phy.contains(&PhyMode::Ax));
    assert_eq!(p.width, Some(ChannelWidth::Mhz160));
    assert_eq!(p.center_channel, Some(23));
    assert_eq!(p.primary_channel, Some(5));
}

#[test]
fn he_with_embedded_vht_info() {
    let params = (1u32 << 14).to_le_bytes();
    let mut body = vec![params[0], params[1], params[2], 0, 0, 0];
    body.extend_from_slice(&[1, 155, 0]); // 80 MHz, center 155
    let ies = cat(&[ext(35, &[0; 20]), ext(36, &body)]);
    let p = parse(&ies, ctx(Band::Ghz5, false));
    assert_eq!(p.width, Some(ChannelWidth::Mhz80));
    assert_eq!(p.center_channel, Some(155));
}

#[test]
fn eht_320_wins_over_he() {
    let he_params = (1u32 << 17).to_le_bytes();
    let mut he = vec![he_params[0], he_params[1], he_params[2], 0, 0, 0];
    he.extend_from_slice(&[1, 0b10, 7, 0, 0]); // HE says 80
    let mut eht = vec![1, 0, 0, 0, 0]; // params: info present, basic MCS
    eht.extend_from_slice(&[4, 31, 31]); // 320 MHz, ccfs0 31, ccfs1 31
    let ies = cat(&[ext(35, &[0; 20]), ext(36, &he), ext(108, &[0; 20]), ext(106, &eht)]);
    let p = parse(&ies, ctx(Band::Ghz6, false));
    assert!(p.phy.contains(&PhyMode::Be));
    assert_eq!(p.width, Some(ChannelWidth::Mhz320));
    assert_eq!(p.width.unwrap().mhz(), 320);
}

#[test]
fn security_variants() {
    let open = parse(&[], ctx(Band::Ghz5, false));
    assert_eq!(open.security, BTreeSet::from([SecurityMode::Open]));

    let wep = parse(&[], ctx(Band::Ghz2_4, true));
    assert_eq!(wep.security, BTreeSet::from([SecurityMode::Wep]));

    let wpa2 = parse(&rsn(&[2]), ctx(Band::Ghz5, true));
    assert_eq!(wpa2.security, BTreeSet::from([SecurityMode::Wpa2Personal]));

    let transition = parse(&rsn(&[2, 8]), ctx(Band::Ghz5, true));
    assert_eq!(
        transition.security,
        BTreeSet::from([SecurityMode::Wpa2Personal, SecurityMode::Wpa3Personal])
    );

    let enterprise = parse(&rsn(&[1]), ctx(Band::Ghz5, true));
    assert_eq!(enterprise.security, BTreeSet::from([SecurityMode::Wpa2Enterprise]));

    let owe = parse(&rsn(&[18]), ctx(Band::Ghz5, false));
    assert_eq!(owe.security, BTreeSet::from([SecurityMode::Owe]));
}

#[test]
fn wpa1_vendor_element_and_mixed_mode() {
    // 00:50:F2 type 1, version 1, TKIP group, one TKIP pairwise, one PSK AKM.
    let wpa1 = el(
        221,
        &[
            0x00, 0x50, 0xF2, 0x01, 1, 0, 0x00, 0x50, 0xF2, 2, 1, 0, 0x00, 0x50, 0xF2, 2, 1, 0,
            0x00, 0x50, 0xF2, 2,
        ],
    );
    let p = parse(&cat(&[wpa1, rsn(&[2])]), ctx(Band::Ghz2_4, true));
    assert_eq!(
        p.security,
        BTreeSet::from([SecurityMode::WpaPersonal, SecurityMode::Wpa2Personal])
    );
}

#[test]
fn malformed_input_never_panics() {
    // Length byte claims more data than exists.
    assert_eq!(parse(&[48, 200, 1, 2], ctx(Band::Ghz5, false)).security.len(), 1);
    // Truncated RSN body.
    let p = parse(&[48, 3, 1, 0, 0], ctx(Band::Ghz5, true));
    assert_eq!(p.security, BTreeSet::from([SecurityMode::Wep]));
    // Lone id byte, empty extension element.
    assert!(parse(&[0], ctx(Band::Ghz5, false)).phy.is_empty());
    assert!(parse(&[255, 0], ctx(Band::Ghz5, false)).phy.is_empty());
    // Sweep: every prefix of a real run must be safe.
    let ies = cat(&[el(45, &[0; 26]), el(61, &[1, 5, 0, 0, 0, 0]), el(192, &[1, 42, 0, 0, 0]), rsn(&[2, 8])]);
    for n in 0..=ies.len() {
        let _ = parse(&ies[..n], ctx(Band::Ghz5, true));
    }
}
