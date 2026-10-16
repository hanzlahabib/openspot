use super::*;
use openspot_model::RadiusPreset;

#[test]
fn several_scans_are_averaged_into_one_point() {
    let mut s = Survey::default();
    s.set_samples_per_point(3);
    s.request_scan(7.0, 9.0);
    for (i, rssi) in [-50, -50, -80].into_iter().enumerate() {
        assert_eq!(s.take_pending_scan(), Some((7.0, 9.0)), "scan {i} requested");
        assert!(s.busy);
        assert!(s.snapshot().points.is_empty(), "nothing recorded before the last scan");
        s.on_scan_result(7.0, 9.0, Ok(vec![ap(1, rssi)]), Vec::new());
    }
    assert_eq!(s.take_pending_scan(), None);
    assert!(!s.busy);
    assert_eq!(s.snapshot().points.len(), 1);
    let p = &s.snapshot().points[0];
    assert_eq!((p.x, p.y), (7.0, 9.0));
    // mean power of -50, -50, -80 dBm = (2e-5 + 1e-8) / 3 mW = -51.8 dBm
    assert_eq!(p.access_points[0].rssi_dbm, -52);
}

#[test]
fn a_failed_scan_discards_the_partial_point() {
    let mut s = Survey::default();
    s.set_samples_per_point(3);
    s.request_scan(1.0, 1.0);
    s.take_pending_scan();
    s.on_scan_result(1.0, 1.0, Ok(vec![ap(1, -50)]), Vec::new());
    s.take_pending_scan();
    s.on_scan_result(1.0, 1.0, Err("boom".into()), Vec::new());
    assert!(s.snapshot().points.is_empty());
    assert!(!s.busy && s.error.is_some() && s.take_pending_scan().is_none());
    // The next measurement starts from scratch.
    s.request_scan(2.0, 2.0);
    s.take_pending_scan();
    s.on_scan_result(2.0, 2.0, Ok(vec![ap(1, -60)]), Vec::new());
    s.take_pending_scan();
    s.on_scan_result(2.0, 2.0, Ok(vec![ap(1, -60)]), Vec::new());
    s.take_pending_scan();
    s.on_scan_result(2.0, 2.0, Ok(vec![ap(1, -60)]), Vec::new());
    assert_eq!(s.snapshot().points.len(), 1);
}

#[test]
fn one_scan_per_point_by_default_and_setting_is_clamped_and_per_zone() {
    let mut s = Survey::default();
    assert_eq!(s.samples_per_point(), 1);
    s.request_scan(0.0, 0.0);
    s.take_pending_scan();
    s.on_scan_result(0.0, 0.0, Ok(vec![ap(1, -50)]), Vec::new());
    assert_eq!(s.snapshot().points.len(), 1);
    s.set_samples_per_point(200);
    assert_eq!(s.samples_per_point(), 10);
    s.set_samples_per_point(0);
    assert_eq!(s.samples_per_point(), 1);
    s.set_samples_per_point(4);
    s.add_zone();
    assert_eq!(s.samples_per_point(), 1, "the new zone keeps its own setting");
}

fn filled_cells(preset: Option<RadiusPreset>) -> usize {
    let mut s = Survey::default();
    s.create_blank_map(100.0, 50.0).unwrap(); // 16 px/m
    s.set_radius_preset(preset);
    for (x, y) in [(400.0, 400.0), (600.0, 400.0), (500.0, 560.0)] {
        s.add_scan(x, y, vec![ap(1, -60)]);
    }
    s.compute_grid().unwrap().values.iter().flatten().count()
}

#[test]
fn bigger_radius_presets_color_more_of_the_plan() {
    let tiny = filled_cells(Some(RadiusPreset::Tiny));
    let medium = filled_cells(Some(RadiusPreset::Medium));
    let extra = filled_cells(Some(RadiusPreset::Extra));
    assert!(tiny < medium && medium < extra, "{tiny} {medium} {extra}");
}

#[test]
fn automatic_radius_is_the_legacy_behaviour() {
    let auto = filled_cells(None);
    // 6 % of the 1600 px long side is 96 px = 6 m at 16 px/m, i.e. the Medium preset.
    assert_eq!(auto, filled_cells(Some(RadiusPreset::Medium)));
}

#[test]
fn radius_change_invalidates_the_heatmap_cache_and_marks_dirty() {
    let mut s = Survey::default();
    let key = s.heat_key();
    s.dirty = false;
    s.set_radius_preset(Some(RadiusPreset::Large));
    assert!(s.dirty);
    assert_ne!(s.heat_key(), key);
    s.dirty = false;
    s.set_radius_preset(Some(RadiusPreset::Large));
    assert!(!s.dirty, "no-op change does not dirty the project");
}

#[test]
fn finished_measurements_ask_for_a_cue_only_when_sound_is_on() {
    use crate::sound::Cue;
    let mut s = Survey::default();
    s.request_scan(1.0, 1.0);
    s.take_pending_scan();
    s.on_scan_result(1.0, 1.0, Ok(vec![ap(1, -50)]), Vec::new());
    assert_eq!(s.take_cue(), None, "sound is opt-in");

    s.sound_enabled = true;
    s.set_samples_per_point(2);
    s.request_scan(2.0, 2.0);
    s.take_pending_scan();
    s.on_scan_result(2.0, 2.0, Ok(vec![ap(1, -50)]), Vec::new());
    assert_eq!(s.take_cue(), None, "an intermediate sample is not a recorded point");
    s.take_pending_scan();
    s.on_scan_result(2.0, 2.0, Ok(vec![ap(1, -50)]), Vec::new());
    assert_eq!(s.take_cue(), Some(Cue::PointRecorded));
    assert_eq!(s.take_cue(), None, "a cue is delivered once");

    s.request_scan(3.0, 3.0);
    s.take_pending_scan();
    s.on_scan_result(3.0, 3.0, Err("boom".into()), Vec::new());
    assert_eq!(s.take_cue(), Some(Cue::ScanFailed));
}

#[test]
fn sound_preference_round_trips_and_defaults_off() {
    let mut s = Survey::default();
    assert!(!s.prefs().sound_enabled);
    s.sound_enabled = true;
    let mut t = Survey::default();
    t.apply_prefs(&s.prefs());
    assert!(t.sound_enabled);
    let old: crate::prefs::SurveyPrefs = serde_json::from_str("{}").unwrap();
    assert!(!old.sound_enabled, "preference files from before this option still load");
}
