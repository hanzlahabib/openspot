use super::*;

fn named(last: u8, ssid: &str, rssi: i32) -> AccessPoint {
    AccessPoint::from_beacon(ssid.into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
}

fn survey() -> Survey {
    let mut s = Survey::default();
    s.add_scan(0.0, 0.0, vec![named(1, "", -50), named(2, "home", -70)]);
    s.add_scan(100.0, 0.0, vec![named(1, "", -60), named(2, "home", -65)]);
    s.add_scan(0.0, 100.0, vec![named(1, "", -55), named(2, "home", -60)]);
    s
}

#[test]
fn excluded_hidden_network_is_gone_from_heatmap_estimates_and_csv() {
    let mut s = survey();
    let hidden = MacAddress([0, 0, 0, 0, 0, 1]);
    assert_eq!(s.hidden_rows().len(), 1);
    assert!(s.hidden_rows()[0].included);
    let before = s.filtered_snapshot().points[0].access_points.len();
    assert_eq!(before, 2);

    s.set_hidden_included(hidden, false);
    let filtered = s.filtered_snapshot();
    assert!(filtered.points.iter().all(|p| p.rssi_of(&hidden).is_none()));
    assert!(filtered.points.iter().all(|p| p.access_points.len() == 1));
    let csv = openspot_export::snapshot_to_csv(s.zone(), &filtered);
    assert!(!csv.contains("00:00:00:00:00:01"), "{csv}");
    assert!(csv.contains("00:00:00:00:00:02"));
    // The stored data is untouched, so the choice is reversible.
    assert_eq!(s.snapshot().points[0].access_points.len(), 2);

    s.selected.insert(hidden);
    assert!(s.ap_estimates().is_empty(), "no estimate for an excluded network");
    s.set_hidden_included(hidden, true);
    assert_eq!(s.ap_estimates().len(), 1);
}

#[test]
fn exclusion_changes_the_heatmap_input() {
    let mut s = survey();
    s.create_blank_map(20.0, 20.0).unwrap();
    let strongest = |s: &Survey| {
        s.compute_grid().unwrap().values.iter().flatten().copied().fold(f32::MIN, f32::max)
    };
    let with = strongest(&s);
    s.set_hidden_included(MacAddress([0, 0, 0, 0, 0, 1]), false);
    let without = strongest(&s);
    assert!(with > without, "{with} vs {without}");
}

#[test]
fn settings_are_per_snapshot_and_survive_save() {
    let mut s = survey();
    let hidden = MacAddress([0, 0, 0, 0, 0, 1]);
    s.set_hidden_included(hidden, false);
    s.set_hidden_label(hidden, "  Camera ");
    assert_eq!(s.hidden_rows()[0].label, "Camera");
    s.add_snapshot();
    assert!(s.hidden_rows().is_empty());
    s.select_snapshot(0);
    let dir = std::env::temp_dir().join(format!("openspot-hidden-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    s.save_to(&dir.join("h.ospot")).unwrap();
    let mut t = Survey::default();
    t.open_from(&dir.join("h.ospot")).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    let row = &t.hidden_rows()[0];
    assert!(!row.included);
    assert_eq!(row.label, "Camera");
    t.set_hidden_label(hidden, "");
    assert_eq!(t.hidden_rows()[0].label, "");
}
