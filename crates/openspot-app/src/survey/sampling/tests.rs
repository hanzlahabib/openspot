use super::*;

fn ap(last: u8, rssi: i32) -> AccessPoint {
    AccessPoint::from_beacon(format!("n{last}"), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
}

fn rssi_of(aps: &[AccessPoint], last: u8) -> Option<i32> {
    aps.iter().find(|a| a.bssid == MacAddress([0, 0, 0, 0, 0, last])).map(|a| a.rssi_dbm)
}

#[test]
fn power_domain_mean_differs_from_db_mean() {
    // (1e-5 + 1e-6) / 2 mW = 5.5e-6 mW -> -52.6 dBm, not the dB mean of -55.
    let avg = average_scans(&[vec![ap(1, -50)], vec![ap(1, -60)]]);
    assert_eq!(rssi_of(&avg, 1), Some(-53));
}

#[test]
fn identical_readings_average_to_themselves() {
    let scans = vec![vec![ap(1, -67)]; 5];
    assert_eq!(rssi_of(&average_scans(&scans), 1), Some(-67));
}

#[test]
fn three_equal_powers_plus_one_much_weaker_barely_moves_the_mean() {
    let avg = average_scans(&[vec![ap(1, -40)], vec![ap(1, -40)], vec![ap(1, -40)], vec![ap(1, -90)]]);
    // mean power ~ 0.75 * 1e-4 mW = -41.25 dBm
    assert_eq!(rssi_of(&avg, 1), Some(-41));
}

#[test]
fn network_seen_in_under_half_the_scans_is_dropped() {
    let scans = [vec![ap(1, -50), ap(2, -80)], vec![ap(1, -50)], vec![ap(1, -50)]];
    let avg = average_scans(&scans);
    assert_eq!(rssi_of(&avg, 1), Some(-50));
    assert_eq!(rssi_of(&avg, 2), None, "seen once in three scans");
}

#[test]
fn network_seen_in_half_the_scans_is_kept_and_averaged_over_sightings_only() {
    let scans = [vec![ap(2, -70)], vec![], vec![ap(2, -70)], vec![]];
    assert_eq!(rssi_of(&average_scans(&scans), 2), Some(-70));
}

#[test]
fn single_scan_is_passed_through_and_empty_input_is_empty() {
    let one = average_scans(&[vec![ap(1, -61), ap(2, -72)]]);
    assert_eq!((rssi_of(&one, 1), rssi_of(&one, 2)), (Some(-61), Some(-72)));
    assert!(average_scans(&[]).is_empty());
}

#[test]
fn averaged_ap_keeps_metadata_and_first_seen_order() {
    let avg = average_scans(&[vec![ap(2, -70), ap(1, -50)], vec![ap(1, -50), ap(2, -70)]]);
    assert_eq!(avg.iter().map(|a| a.bssid.0[5]).collect::<Vec<_>>(), [2, 1]);
    assert_eq!(avg[0].ssid, "n2");
    assert_eq!(avg[0].channel, 6);
}

#[test]
fn sampler_completes_after_the_requested_number_of_scans() {
    let mut s = Sampler::new(1.0, 2.0, 3);
    assert!(!s.push(vec![ap(1, -50)]));
    assert!(!s.push(vec![ap(1, -50)]));
    assert_eq!(s.progress(), (2, 3));
    assert!(s.push(vec![ap(1, -50)]));
    assert_eq!(rssi_of(&s.finish(), 1), Some(-50));
}

#[test]
fn sampler_clamps_the_requested_count() {
    assert_eq!(Sampler::new(0.0, 0.0, 0).progress().1, 1);
    assert_eq!(Sampler::new(0.0, 0.0, 200).progress().1, usize::from(MAX_SAMPLES_PER_POINT));
}

#[test]
fn presets_need_a_scale_otherwise_the_automatic_fraction_applies() {
    let scale = Scale::new(20.0);
    assert_eq!(extend_px(Some(RadiusPreset::Medium), scale, 1600.0), 120.0);
    assert_eq!(extend_px(Some(RadiusPreset::Tiny), scale, 1600.0), 30.0);
    assert_eq!(extend_px(Some(RadiusPreset::Medium), None, 1600.0), 1600.0 * AUTO_EXTEND_FRACTION);
    assert_eq!(extend_px(None, scale, 1600.0), 1600.0 * AUTO_EXTEND_FRACTION);
}
