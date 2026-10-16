use super::*;

fn sample_project() -> Project {
    // 802.11ac AP with real-looking IEs so the reload re-parse is exercised.
    let ies = vec![45, 0, 61, 6, 36, 0b101, 0, 0, 0, 0, 191, 0, 192, 5, 1, 42, 0, 0, 0];
    let ap = |last: u8, rssi: i32| {
        AccessPoint::from_beacon("Net".into(), MacAddress([2, 0, 0, 0, 0, last]), rssi, 5_180_000, true, 100, ies.clone())
    };
    let mut p = Project::new("House");
    let zone = &mut p.zones[0];
    zone.map = Some(MapImage { original_name: "plan.png".into(), data: vec![1, 2, 3, 4], width: 800, height: 600 });
    zone.scale = Scale::new(42.5);
    zone.snapshots[0].points.push(ScanPoint::new(10.5, 20.25, 1_700_000_000, vec![ap(1, -50), ap(2, -71)]));
    zone.snapshots[0].points.push(ScanPoint::new(300.0, 100.0, 1_700_000_060, vec![ap(1, -63)]));
    zone.snapshots.push(Snapshot::new("After moving AP"));
    p.zones.push(Zone::new("Upstairs"));
    p
}

#[test]
fn round_trip_preserves_everything() {
    let project = sample_project();
    let mut store = Store::in_memory().unwrap();
    store.save(&project).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded, project);
    // IEs were re-parsed on load.
    let ap = &loaded.zones[0].snapshots[0].points[0].access_points[0];
    assert_eq!(ap.width, Some(openspot_wlan::ChannelWidth::Mhz80));
}

#[test]
fn save_replaces_previous_contents() {
    let mut store = Store::in_memory().unwrap();
    store.save(&sample_project()).unwrap();
    let second = Project::new("Other");
    store.save(&second).unwrap();
    assert_eq!(store.load().unwrap(), second);
}

#[test]
fn empty_store_reports_empty() {
    assert!(matches!(Store::in_memory().unwrap().load(), Err(StoreError::Empty)));
}

#[test]
fn file_round_trip_and_reopen() {
    let dir = std::env::temp_dir().join(format!("openspot-store-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.ospot");
    let project = sample_project();
    Store::open(&path).unwrap().save(&project).unwrap();
    assert_eq!(Store::open(&path).unwrap().load().unwrap(), project);
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn newer_schema_is_rejected() {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
    assert!(matches!(Store::init(conn), Err(StoreError::UnsupportedVersion(_))));
}

#[test]
fn legacy_v1_file_is_upgraded_in_place() {
    let dir = std::env::temp_dir().join(format!("openspot-store-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.ospot");
    {
        // A file as written by the first release: base schema only, user_version 1.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(migrations::ALL[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let mut store = Store::open(&path).unwrap();
    let version: i32 = store.conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    let project = sample_project();
    store.save(&project).unwrap();
    assert_eq!(Store::open(&path).unwrap().load().unwrap(), project);
    // Windows refuses to delete a file that is still open.
    drop(store);
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn structure_round_trips() {
    let mut p = sample_project();
    let b = p.structure.add_building("Main");
    let (z0, z1) = (p.zones[0].id, p.zones[1].id);
    p.structure.move_zone_to_building(z1, b);
    p.structure.move_zone_to_building(z0, b);
    let f = p.structure.floor_mut(z0).unwrap();
    f.label = "Roof".into();
    f.height_m = 2.5;
    f.alignment = openspot_model::FloorAlignment { dx_m: 1.5, dy_m: -2.0, rotation_rad: 0.25 };
    p.structure.stamp_saved(1_700_000_500);
    let mut store = Store::in_memory().unwrap();
    store.save(&p).unwrap();
    assert_eq!(store.load().unwrap(), p);
    let s = store.summary().unwrap();
    assert_eq!((s.name.as_str(), s.created, s.changed, s.zone_count), ("House", 1_700_000_500, 1_700_000_500, 2));
    // Re-saving fully replaces the structure.
    p.structure.remove_building(b);
    store.save(&p).unwrap();
    assert!(store.load().unwrap().structure.buildings.is_empty());
}

#[test]
fn project_saved_before_structure_existed_upgrades_with_defaults() {
    // A file at the schema version just before the structure slot.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let before = crate::migrations::STRUCTURE_SLOT;
    for (i, sql) in crate::migrations::ALL.iter().enumerate().take(before) {
        conn.execute_batch(sql).unwrap();
        conn.pragma_update(None, "user_version", i as i32 + 1).unwrap();
    }
    conn.execute("INSERT INTO project (id, name) VALUES (?1, 'Old')", [Uuid::new_v4().to_string()]).unwrap();
    let store = Store::init(conn).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.name, "Old");
    assert_eq!(loaded.structure, openspot_model::Structure::default());
    assert_eq!(store.summary().unwrap().created, 0);
}

fn aliased_project() -> Project {
    let mut p = sample_project();
    p.aliases.by_bssid.insert(MacAddress([2, 0, 0, 0, 0, 1]), "Lobby".into());
    p.aliases.by_bssid.insert(MacAddress([0xAA, 0xBB, 0xCC, 1, 2, 3]), "Büro «2»".into());
    p
}

#[test]
fn project_aliases_round_trip() {
    let p = aliased_project();
    let mut store = Store::in_memory().unwrap();
    store.save(&p).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.aliases, p.aliases);
    assert_eq!(loaded, p);
}

#[test]
fn project_aliases_travel_with_the_file() {
    let dir = std::env::temp_dir().join(format!("openspot-store-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.ospot");
    let p = aliased_project();
    Store::open(&path).unwrap().save(&p).unwrap();
    // A different process (a colleague's machine) only has the file.
    assert_eq!(Store::open(&path).unwrap().load().unwrap().aliases, p.aliases);
    let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
}

#[test]
fn resaving_replaces_aliases_instead_of_accumulating_them() {
    let mut p = aliased_project();
    let mut store = Store::in_memory().unwrap();
    store.save(&p).unwrap();
    p.aliases.by_bssid.remove(&MacAddress([2, 0, 0, 0, 0, 1]));
    p.aliases.by_bssid.insert(MacAddress([9; 6]), "New".into());
    store.save(&p).unwrap();
    assert_eq!(store.load().unwrap().aliases, p.aliases);
    store.save(&Project::new("Empty")).unwrap();
    assert!(store.load().unwrap().aliases.is_empty());
}

#[test]
fn aliases_do_not_leak_between_projects_in_one_file() {
    // The file holds one project at a time; saving another must not inherit the old aliases.
    let mut store = Store::in_memory().unwrap();
    store.save(&aliased_project()).unwrap();
    let other = Project::new("Other");
    store.save(&other).unwrap();
    assert_eq!(store.load().unwrap(), other);
}

#[test]
fn project_saved_before_aliases_existed_upgrades_with_none() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let before = crate::migrations::SPARE_SLOT;
    for (i, sql) in crate::migrations::ALL.iter().enumerate().take(before) {
        conn.execute_batch(sql).unwrap();
        conn.pragma_update(None, "user_version", i as i32 + 1).unwrap();
    }
    conn.execute("INSERT INTO project (id, name) VALUES (?1, 'Old')", [Uuid::new_v4().to_string()]).unwrap();
    let mut store = Store::init(conn).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!((loaded.name.as_str(), loaded.aliases.is_empty()), ("Old", true));
    // And the upgraded file can now store aliases.
    let p = aliased_project();
    store.save(&p).unwrap();
    assert_eq!(store.load().unwrap().aliases, p.aliases);
}

fn linked_project() -> Project {
    let mut p = sample_project();
    p.zones[0].snapshots[0].points[0].link =
        Some(openspot_model::LinkInfo { bssid: MacAddress([2, 0, 0, 0, 0, 1]), tx_rate_mbps: 866.7, rx_rate_mbps: 780.0 });
    // Second point: connected but the driver reported no receive rate.
    p.zones[0].snapshots[0].points[1].link =
        Some(openspot_model::LinkInfo { bssid: MacAddress([2, 0, 0, 0, 0, 2]), tx_rate_mbps: 54.0, rx_rate_mbps: 0.0 });
    p
}

#[test]
fn link_info_round_trips_per_point_and_none_stays_none() {
    let mut p = linked_project();
    p.zones[0].snapshots[1].points.push(ScanPoint::new(1.0, 1.0, 5, vec![])); // never connected
    let mut store = Store::in_memory().unwrap();
    store.save(&p).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded, p);
    let points = &loaded.zones[0].snapshots[0].points;
    assert_eq!(points[0].link.as_ref().map(|l| l.tx_rate_mbps), Some(866.7));
    assert_eq!(points[1].link.as_ref().map(|l| (l.tx_rate_mbps, l.rx_rate_mbps)), Some((54.0, 0.0)));
    assert!(loaded.zones[0].snapshots[1].points[0].link.is_none());
}

#[test]
fn removed_points_leave_no_link_rows_behind() {
    let mut p = linked_project();
    let mut store = Store::in_memory().unwrap();
    store.save(&p).unwrap();
    p.zones[0].snapshots[0].points.remove(0);
    store.save(&p).unwrap();
    let rows: i64 = store.conn.query_row("SELECT COUNT(*) FROM scan_link", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
    assert_eq!(store.load().unwrap(), p);
}

#[test]
fn project_saved_before_link_info_existed_loads_without_links() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let before = crate::migrations::SPARE_SLOT;
    for (i, sql) in crate::migrations::ALL.iter().enumerate().take(before) {
        conn.execute_batch(sql).unwrap();
        conn.pragma_update(None, "user_version", i as i32 + 1).unwrap();
    }
    let pid = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO project (id, name) VALUES (?1, 'Old')", [&pid]).unwrap();
    let (zid, sid, point) = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
    conn.execute("INSERT INTO zone (id, project_id, position, name) VALUES (?1, ?2, 0, 'Z')", [&zid, &pid]).unwrap();
    conn.execute("INSERT INTO snapshot (id, zone_id, position, name) VALUES (?1, ?2, 0, 'S')", [&sid, &zid]).unwrap();
    conn.execute("INSERT INTO scan_point (id, snapshot_id, position, x, y, time) VALUES (?1, ?2, 0, 1.0, 2.0, 3)", [&point, &sid]).unwrap();
    let mut store = Store::init(conn).unwrap();
    let loaded = store.load().unwrap();
    let p = &loaded.zones[0].snapshots[0].points[0];
    assert_eq!((p.x, p.link.is_none()), (1.0, true));
    // The upgraded file can record links from now on.
    let mut upgraded = loaded;
    upgraded.zones[0].snapshots[0].points[0].link =
        Some(openspot_model::LinkInfo { bssid: MacAddress([2; 6]), tx_rate_mbps: 100.0, rx_rate_mbps: 90.0 });
    store.save(&upgraded).unwrap();
    assert_eq!(store.load().unwrap(), upgraded);
}
