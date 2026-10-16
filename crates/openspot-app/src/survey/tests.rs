use super::*;

fn ap(last: u8, rssi: i32) -> AccessPoint {
    AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
}

#[test]
fn scans_accumulate_and_undo() {
    let mut s = Survey::default();
    assert!(!s.undo());
    s.add_scan(10.0, 20.0, vec![ap(1, -50)]);
    s.add_scan(30.0, 40.0, vec![ap(1, -60)]);
    assert_eq!(s.snapshot().points.len(), 2);
    assert!(s.dirty);
    assert!(s.undo());
    assert_eq!(s.snapshot().points.len(), 1);
    assert_eq!(s.snapshot().points[0].x, 10.0);
}

#[test]
fn redo_restores_and_new_edits_drop_redo() {
    let mut s = Survey::default();
    s.add_scan(1.0, 1.0, vec![ap(1, -50)]);
    s.add_scan(2.0, 2.0, vec![ap(1, -60)]);
    assert!(s.undo() && s.undo());
    assert!(s.snapshot().points.is_empty());
    assert!(s.can_redo() && !s.can_undo());
    assert!(s.redo());
    assert_eq!(s.snapshot().points.len(), 1);
    assert_eq!(s.snapshot().points[0].x, 1.0);
    s.add_scan(9.0, 9.0, vec![]);
    assert!(!s.can_redo());
    assert!(!s.redo());
    assert_eq!(s.snapshot().points.len(), 2);
}

#[test]
fn scale_changes_are_undoable() {
    let mut s = Survey::default();
    s.set_scale(Scale::new(10.0).unwrap());
    s.set_scale(Scale::new(20.0).unwrap());
    assert!(s.undo());
    assert_eq!(s.zone().scale.unwrap().pixels_per_meter(), 10.0);
    assert!(s.undo());
    assert!(s.zone().scale.is_none());
    assert!(s.redo() && s.redo());
    assert_eq!(s.zone().scale.unwrap().pixels_per_meter(), 20.0);
}

#[test]
fn undo_survives_switching_snapshots_and_clears_on_map_load() {
    let mut s = Survey::default();
    s.add_scan(1.0, 1.0, vec![]);
    s.add_snapshot();
    s.add_scan(2.0, 2.0, vec![]);
    s.select_snapshot(0);
    // Undoing removes the point from the snapshot it was recorded in, not the current one.
    assert!(s.undo());
    assert_eq!(s.zone().snapshots[1].points.len(), 0);
    assert_eq!(s.zone().snapshots[0].points.len(), 1);
    s.create_blank_map(10.0, 10.0).unwrap();
    assert!(!s.can_undo() && !s.can_redo());
}

#[test]
fn zones_and_snapshots_switch_and_reset_selection() {
    let mut s = Survey::default();
    s.selected.insert(MacAddress([1; 6]));
    s.add_zone();
    assert_eq!((s.zone, s.snapshot), (1, 0));
    assert!(s.selected.is_empty());
    s.add_snapshot();
    assert_eq!(s.snapshot, 1);
    s.select_zone(99); // out of range: ignored
    assert_eq!(s.zone, 1);
}

#[test]
fn save_and_open_round_trip_through_a_file() {
    let dir = std::env::temp_dir().join(format!("openspot-app-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.ospot");
    let mut s = Survey::default();
    s.add_scan(5.0, 6.0, vec![ap(7, -44)]);
    s.set_scale(Scale::new(20.0).unwrap());
    s.save_to(&path).unwrap();
    assert!(!s.dirty);

    let mut t = Survey::default();
    t.open_from(&path).unwrap();
    assert_eq!(t.project, s.project);
    assert_eq!(t.path.as_deref(), Some(path.as_path()));
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn open_of_garbage_reports_an_error_and_keeps_state() {
    let dir = std::env::temp_dir().join(format!("openspot-app-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.ospot");
    std::fs::write(&path, b"this is not sqlite").unwrap();
    let mut s = Survey::default();
    s.add_scan(1.0, 1.0, vec![ap(1, -50)]);
    assert!(s.open_from(&path).is_err());
    assert_eq!(s.snapshot().points.len(), 1);
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn load_map_decodes_dimensions_and_clears_scale() {
    let dir = std::env::temp_dir().join(format!("openspot-app-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plan.png");
    image::RgbaImage::from_pixel(64, 32, image::Rgba([255, 255, 255, 255])).save(&path).unwrap();
    let mut s = Survey::default();
    s.set_scale(Scale::new(10.0).unwrap());
    s.load_map(&path).unwrap();
    let m = s.zone().map.as_ref().unwrap();
    assert_eq!((m.width, m.height, m.original_name.as_str()), (64, 32, "plan.png"));
    assert!(s.zone().scale.is_none());
    assert!(s.load_map(&dir.join("missing.png")).is_err());
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn grid_needs_map_and_points() {
    let mut s = Survey::default();
    assert!(s.compute_grid().is_none());
    let dir = std::env::temp_dir().join(format!("openspot-app-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.png");
    image::RgbaImage::from_pixel(160, 160, image::Rgba([0, 0, 0, 255])).save(&path).unwrap();
    s.load_map(&path).unwrap();
    assert!(s.compute_grid().is_none());
    for (x, y, r) in [(20.0, 20.0, -50), (140.0, 20.0, -60), (80.0, 140.0, -70)] {
        s.add_scan(x, y, vec![ap(1, r)]);
    }
    let g = s.compute_grid().unwrap();
    assert!(g.values.iter().flatten().count() > 100);
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn fake_provider_survey_end_to_end() {
    use openspot_wlan::{FakeProvider, ScanOptions, WlanProvider};
    let dir = std::env::temp_dir().join(format!("openspot-app-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plan.png");
    image::RgbaImage::from_pixel(200, 120, image::Rgba([255, 255, 255, 255])).save(&path).unwrap();

    let provider = FakeProvider;
    let adapter = provider.adapters().unwrap().remove(0);
    let mut s = Survey::default();
    s.load_map(&path).unwrap();
    for (i, (x, y)) in [(30.0, 30.0), (170.0, 30.0), (100.0, 100.0), (60.0, 80.0)].into_iter().enumerate() {
        let aps = provider.scan(&adapter, &ScanOptions { probes: 1 + i as u8, ..Default::default() }).unwrap();
        s.add_scan(x, y, aps);
    }
    assert_eq!(s.snapshot().networks().len(), 3);

    for choice in [MetricChoice::BestSignal, MetricChoice::ApCount, MetricChoice::SelectedSignal] {
        s.metric_choice = choice;
        s.selected = s.snapshot().networks().iter().map(|n| n.0).take(1).collect();
        let grid = s.compute_grid().expect("grid");
        let px = s.metric().gradient().render_rgba(&grid, 200);
        assert_eq!(px.len(), grid.cols * grid.rows * 4);
        assert!(px.chunks(4).any(|p| p[3] == 200));
    }
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn blank_plan_sets_matching_scale() {
    let mut s = Survey::default();
    s.create_blank_map(20.0, 10.0).unwrap();
    let m = s.zone().map.as_ref().unwrap();
    assert_eq!((m.width, m.height), (1600, 800));
    let scale = s.zone().scale.unwrap();
    assert!((scale.pixels_per_meter() - 80.0).abs() < 1e-9);
    assert!((scale.to_meters(f64::from(m.width)) - 20.0).abs() < 1e-9);
    assert!(image::load_from_memory(&m.data).is_ok());
}

#[test]
fn blank_plan_rejects_bad_sizes() {
    let mut s = Survey::default();
    for (w, h) in [(0.0, 5.0), (5.0, -1.0), (f64::NAN, 5.0), (5000.0, 5.0)] {
        assert!(s.create_blank_map(w, h).is_err(), "{w}x{h}");
    }
    assert!(s.zone().map.is_none());
}

#[test]
fn troubleshooting_wraps_only_supported_metrics_and_resets_thresholds() {
    let mut s = Survey { troubleshoot: true, ..Survey::default() };
    assert!(matches!(s.metric(), Metric::Troubleshoot { .. }));
    s.threshold_acceptable = -50;
    s.reset_thresholds();
    assert_eq!((s.threshold_acceptable, s.threshold_critical), (-67, -75));

    s.metric_choice = MetricChoice::ApCount;
    assert!(!s.supports_troubleshooting());
    assert!(matches!(s.metric(), Metric::ApCount { .. }));

    s.metric_choice = MetricChoice::Sir;
    s.reset_thresholds();
    assert_eq!((s.threshold_acceptable, s.threshold_critical), (15, 10));
    assert_eq!(s.base_metric_unit(), "dB");
}

#[test]
fn hover_text_reads_the_cached_grid() {
    use openspot_heatmap::Grid;
    let mut s = Survey::default();
    assert!(s.heat_text_at(5.0, 5.0).is_none(), "no grid yet");
    s.textures.heat_grid = Some(Grid { cols: 2, rows: 1, cell_px: 10.0, values: vec![Some(-61.4), None] });
    assert_eq!(s.heat_text_at(5.0, 5.0).as_deref(), Some("-61 dBm"));
    assert!(s.heat_text_at(15.0, 5.0).is_none(), "hole");
    assert!(s.heat_text_at(25.0, 5.0).is_none(), "outside grid");
    assert!(s.heat_text_at(-1.0, 5.0).is_none());
    s.show_heatmap = false;
    assert!(s.heat_text_at(5.0, 5.0).is_none());

    s.show_heatmap = true;
    s.metric_choice = MetricChoice::BandCoverage;
    s.textures.heat_grid = Some(Grid { cols: 1, rows: 1, cell_px: 10.0, values: vec![Some(2.0)] });
    assert_eq!(s.heat_text_at(1.0, 1.0).as_deref(), Some("5 GHz only"));
}

#[test]
fn ap_estimates_cover_selected_networks_with_enough_points() {
    let mut s = Survey::default();
    let bssid = MacAddress([0, 0, 0, 0, 0, 1]);
    for (x, y, r) in [(0.0, 0.0, -40), (50.0, 0.0, -50), (0.0, 50.0, -55), (90.0, 90.0, -80)] {
        s.add_scan(x, y, vec![ap(1, r)]);
    }
    assert!(s.ap_estimates().is_empty(), "nothing selected");
    s.selected.insert(bssid);
    let est = s.ap_estimates();
    assert_eq!(est.len(), 1);
    assert_eq!(est[0].0, bssid);
    s.selected.insert(MacAddress([7; 6]));
    assert_eq!(s.ap_estimates().len(), 1, "unknown network is skipped");
}

#[test]
fn prefs_round_trip_and_clamp() {
    let s = Survey {
        gradient: GradientPreset::BlueRed,
        smoothing: Smoothing::Extra,
        opacity: 99,
        show_points: false,
        ..Survey::default()
    };
    let p = s.prefs();
    let mut t = Survey::default();
    t.apply_prefs(&p);
    assert_eq!((t.gradient, t.smoothing, t.opacity, t.show_points), (GradientPreset::BlueRed, Smoothing::Extra, 99, false));
    t.apply_prefs(&SurveyPrefs { gradient: 99, smoothing: 99, opacity: 0, ..SurveyPrefs::default() });
    assert_eq!((t.gradient, t.smoothing, t.opacity), (GradientPreset::Default, Smoothing::Normal, MIN_OPACITY));
        t.apply_prefs(&SurveyPrefs { unit: 42, ..SurveyPrefs::default() });
        assert_eq!(t.unit, LengthUnit::Meters);
}

#[test]
fn rename_rejects_blank_and_trims() {
    let mut s = Survey::default();
    assert!(!s.rename_zone("   "));
    assert!(s.rename_zone("  Ground floor "));
    assert_eq!(s.zone().name, "Ground floor");
    assert!(!s.rename_snapshot(""));
    assert!(s.rename_snapshot("Before move"));
    assert_eq!(s.snapshot().name, "Before move");
    assert!(s.dirty);
}

#[test]
fn a_project_always_keeps_one_zone_and_one_snapshot() {
    let mut s = Survey::default();
    assert!(!s.delete_zone());
    assert!(!s.delete_snapshot());
    assert_eq!((s.project.zones.len(), s.zone().snapshots.len()), (1, 1));
}

#[test]
fn deleting_zone_and_snapshot_keeps_selection_valid_and_clears_history() {
    let mut s = Survey::default();
    s.add_scan(1.0, 1.0, vec![]);
    s.add_zone();
    s.add_zone();
    assert_eq!(s.zone, 2);
    assert!(s.delete_zone());
    assert_eq!((s.zone, s.project.zones.len()), (1, 2));
    assert!(!s.can_undo(), "indices shifted, so history is dropped");
    s.select_zone(0);
    s.add_snapshot();
    s.add_snapshot();
    assert_eq!(s.snapshot, 2);
    assert!(s.delete_snapshot());
    assert_eq!((s.snapshot, s.zone().snapshots.len()), (1, 2));
    s.select_snapshot(0);
    assert!(s.delete_snapshot());
    assert_eq!((s.snapshot, s.zone().snapshots.len()), (0, 1));
}

#[test]
fn saving_over_an_existing_file_keeps_the_previous_version_as_bak() {
    let dir = std::env::temp_dir().join(format!("openspot-app-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.ospot");
    let bak = dir.join("p.ospot.bak");

    let mut s = Survey::default();
    s.save_to(&path).unwrap();
    assert!(!bak.exists(), "first save has nothing to back up");

    s.add_scan(1.0, 2.0, vec![ap(1, -50)]);
    s.save_to(&path).unwrap();
    assert!(bak.exists());

    // The backup is the version from before the second save: no measurement points yet.
    let mut old = Survey::default();
    old.open_from(&bak).unwrap();
    assert!(old.snapshot().points.is_empty());
    let mut new = Survey::default();
    new.open_from(&path).unwrap();
    assert_eq!(new.snapshot().points.len(), 1);
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn project_aliases_survive_save_and_open_and_mark_the_project_changed() {
    use crate::aliases::{self, AliasScope, Aliases};
    let dir = std::env::temp_dir().join(format!("openspot-app-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.ospot");
    let bssid = MacAddress([0xAA, 0, 0, 0, 0, 7]);

    let mut s = Survey::default();
    s.save_to(&path).unwrap();
    assert!(!s.dirty);
    let mut app = Aliases::default();
    let mut edited = aliases::active(AliasScope::Project, &app, &s.project.aliases);
    edited.set(&bssid, "Server room");
    if aliases::commit(AliasScope::Project, edited, &mut app, &mut s.project.aliases) {
        s.mark_dirty();
    }
    assert!(s.dirty, "a project alias edit is an unsaved change");
    assert!(app.is_empty(), "the app-wide list is untouched");
    s.save_to(&path).unwrap();

    let mut t = Survey::default();
    t.open_from(&path).unwrap();
    let shown = aliases::active(AliasScope::Project, &Aliases::default(), &t.project.aliases);
    assert_eq!(shown.get(&bssid), Some("Server room"));
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}
