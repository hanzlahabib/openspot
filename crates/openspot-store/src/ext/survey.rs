//! Survey canvas data: paths, sampling settings, manual AP positions, drawings and similar.

mod drawing;

use std::collections::BTreeSet;

use rusqlite::{Connection, Transaction, params};

use openspot_wlan::MacAddress;
use openspot_model::{CropArea, MAX_SAMPLES_PER_POINT, Project, Provenance, RadiusPreset, SurveyPath, ZoneCanvas};

use crate::{StoreError, parse_uuid};

/// DDL for this feature's migration.
pub(crate) const SQL: &str = "
CREATE TABLE canvas_path (
    snapshot_id TEXT PRIMARY KEY REFERENCES snapshot(id) ON DELETE CASCADE,
    interval_m REAL NOT NULL);
CREATE TABLE canvas_waypoint (
    snapshot_id TEXT NOT NULL REFERENCES snapshot(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, x REAL NOT NULL, y REAL NOT NULL,
    PRIMARY KEY (snapshot_id, position));
CREATE TABLE canvas_hidden (
    snapshot_id TEXT NOT NULL REFERENCES snapshot(id) ON DELETE CASCADE,
    bssid BLOB NOT NULL, excluded INTEGER NOT NULL, label TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, bssid));
CREATE TABLE canvas_manual_ap (
    snapshot_id TEXT NOT NULL REFERENCES snapshot(id) ON DELETE CASCADE,
    bssid BLOB NOT NULL, x REAL NOT NULL, y REAL NOT NULL,
    PRIMARY KEY (snapshot_id, bssid));
CREATE TABLE canvas_provenance (
    snapshot_id TEXT NOT NULL REFERENCES snapshot(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, source_name TEXT NOT NULL,
    points_in INTEGER NOT NULL, points_kept INTEGER NOT NULL,
    PRIMARY KEY (snapshot_id, position));
CREATE TABLE canvas_drawing (
    zone_id TEXT PRIMARY KEY REFERENCES zone(id) ON DELETE CASCADE,
    width INTEGER NOT NULL, height INTEGER NOT NULL);
CREATE TABLE canvas_shape (
    zone_id TEXT NOT NULL REFERENCES zone(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, id TEXT NOT NULL, kind INTEGER NOT NULL,
    thickness INTEGER NOT NULL, font INTEGER NOT NULL, text TEXT, filled INTEGER NOT NULL,
    PRIMARY KEY (zone_id, position));
CREATE TABLE canvas_shape_vertex (
    zone_id TEXT NOT NULL, shape_position INTEGER NOT NULL, position INTEGER NOT NULL,
    x REAL NOT NULL, y REAL NOT NULL,
    PRIMARY KEY (zone_id, shape_position, position),
    FOREIGN KEY (zone_id, shape_position) REFERENCES canvas_shape(zone_id, position) ON DELETE CASCADE);
CREATE TABLE canvas_zone (
    zone_id TEXT PRIMARY KEY REFERENCES zone(id) ON DELETE CASCADE,
    radius_code INTEGER, samples_per_point INTEGER NOT NULL, gray_unmeasured INTEGER NOT NULL,
    crop_kind INTEGER);
CREATE TABLE canvas_crop_vertex (
    zone_id TEXT NOT NULL REFERENCES zone(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, x REAL NOT NULL, y REAL NOT NULL,
    PRIMARY KEY (zone_id, position));
";

/// `canvas_zone.crop_kind` codes; stored values, never renumber.
const CROP_RECT: i64 = 0;
const CROP_POLYGON: i64 = 1;

/// Writes this feature's data inside the save transaction (after the core tables).
pub(crate) fn save(tx: &Transaction<'_>, project: &Project) -> Result<(), StoreError> {
    for (id, data) in project.canvas.snapshots() {
        let sid = id.to_string();
        tx.execute("INSERT INTO canvas_path (snapshot_id, interval_m) VALUES (?1, ?2)", params![sid, data.path.interval_m])?;
        for (i, p) in data.provenance.iter().enumerate() {
            tx.execute(
                "INSERT INTO canvas_provenance (snapshot_id, position, source_name, points_in, points_kept) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![sid, i, p.source_name, p.points_in, p.points_kept],
            )?;
        }
        for (bssid, (x, y)) in &data.manual_aps {
            tx.execute(
                "INSERT INTO canvas_manual_ap (snapshot_id, bssid, x, y) VALUES (?1, ?2, ?3, ?4)",
                params![sid, bssid.0.as_slice(), x, y],
            )?;
        }
        let hidden: BTreeSet<&MacAddress> = data.hidden_excluded.iter().chain(data.hidden_labels.keys()).collect();
        for bssid in hidden {
            tx.execute(
                "INSERT INTO canvas_hidden (snapshot_id, bssid, excluded, label) VALUES (?1, ?2, ?3, ?4)",
                params![sid, bssid.0.as_slice(), data.hidden_excluded.contains(bssid), data.hidden_labels.get(bssid).map_or("", String::as_str)],
            )?;
        }
        for (i, (x, y)) in data.path.waypoints.iter().enumerate() {
            tx.execute("INSERT INTO canvas_waypoint (snapshot_id, position, x, y) VALUES (?1, ?2, ?3, ?4)", params![sid, i, x, y])?;
        }
    }
    for (id, data) in project.canvas.zones() {
        let zid = id.to_string();
        let (kind, vertices): (Option<i64>, Vec<(f64, f64)>) = match &data.crop {
            None => (None, Vec::new()),
            Some(CropArea::Rect { min, max }) => (Some(CROP_RECT), vec![*min, *max]),
            Some(CropArea::Polygon(pts)) => (Some(CROP_POLYGON), pts.clone()),
        };
        tx.execute(
            "INSERT INTO canvas_zone (zone_id, radius_code, samples_per_point, gray_unmeasured, crop_kind) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![zid, data.radius.map(RadiusPreset::code), data.samples_per_point, data.gray_unmeasured, kind],
        )?;
        for (i, (x, y)) in vertices.iter().enumerate() {
            tx.execute("INSERT INTO canvas_crop_vertex (zone_id, position, x, y) VALUES (?1, ?2, ?3, ?4)", params![zid, i, x, y])?;
        }
    }
    drawing::save(tx, project)?;
    Ok(())
}

/// Reads this feature's data into `project` (after the core tables are loaded).
pub(crate) fn load(conn: &Connection, project: &mut Project) -> Result<(), StoreError> {
    let mut paths = conn.prepare("SELECT snapshot_id, interval_m FROM canvas_path")?;
    let rows = paths.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    let mut points = conn.prepare("SELECT x, y FROM canvas_waypoint WHERE snapshot_id = ?1 ORDER BY position")?;
    for (sid, interval_m) in rows {
        let waypoints = points.query_map([&sid], |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        let id = parse_uuid(&sid)?;
        project.canvas.snapshot_mut(id).path = SurveyPath { waypoints, interval_m };
    }
    let mut hidden = conn.prepare("SELECT snapshot_id, bssid, excluded, label FROM canvas_hidden")?;
    let rows = hidden
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (sid, bssid, excluded, label) in rows {
        let bssid = MacAddress(<[u8; 6]>::try_from(bssid.as_slice()).map_err(|_| StoreError::Corrupt("hidden bssid is not 6 bytes"))?);
        let snap = project.canvas.snapshot_mut(parse_uuid(&sid)?);
        if excluded {
            snap.hidden_excluded.insert(bssid);
        }
        if !label.is_empty() {
            snap.hidden_labels.insert(bssid, label);
        }
    }
    let mut aps = conn.prepare("SELECT snapshot_id, bssid, x, y FROM canvas_manual_ap")?;
    let rows = aps
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (sid, bssid, x, y) in rows {
        let bssid = MacAddress(<[u8; 6]>::try_from(bssid.as_slice()).map_err(|_| StoreError::Corrupt("AP bssid is not 6 bytes"))?);
        project.canvas.snapshot_mut(parse_uuid(&sid)?).manual_aps.insert(bssid, (x, y));
    }
    let mut prov = conn.prepare("SELECT snapshot_id, source_name, points_in, points_kept FROM canvas_provenance ORDER BY snapshot_id, position")?;
    let rows = prov
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, usize>(2)?, r.get::<_, usize>(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (sid, source_name, points_in, points_kept) in rows {
        project.canvas.snapshot_mut(parse_uuid(&sid)?).provenance.push(Provenance { source_name, points_in, points_kept });
    }
    let mut zones = conn.prepare("SELECT zone_id, radius_code, samples_per_point, gray_unmeasured, crop_kind FROM canvas_zone")?;
    let rows = zones
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, i64>(2)?, r.get::<_, bool>(3)?, r.get::<_, Option<i64>>(4)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut vertices = conn.prepare("SELECT x, y FROM canvas_crop_vertex WHERE zone_id = ?1 ORDER BY position")?;
    for (zid, radius, samples, gray_unmeasured, crop_kind) in rows {
        let pts = vertices.query_map([&zid], |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        let crop = match crop_kind {
            Some(CROP_RECT) if pts.len() == 2 => Some(CropArea::rect(pts[0], pts[1])),
            Some(CROP_POLYGON) => CropArea::polygon(pts),
            _ => None,
        };
        project.canvas.set_zone(
            parse_uuid(&zid)?,
            ZoneCanvas {
                radius: radius.and_then(RadiusPreset::from_code),
                samples_per_point: samples.clamp(1, i64::from(MAX_SAMPLES_PER_POINT)) as u8,
                crop,
                gray_unmeasured,
                drawing: None,
            },
        );
    }
    drawing::load(conn, project)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use openspot_model::Project;

    #[test]
    fn path_round_trips_in_order() {
        let mut p = Project::new("p");
        let sid = p.zones[0].snapshots[0].id;
        let path = &mut p.canvas.snapshot_mut(sid).path;
        path.waypoints = vec![(1.0, 2.0), (30.5, 4.25), (-3.0, 9.0)];
        path.interval_m = 1.5;
        let mut store = Store::in_memory().unwrap();
        store.save(&p).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.canvas.snapshot(sid).unwrap().path, p.canvas.snapshot(sid).unwrap().path);
        assert_eq!(loaded, p);
    }

    #[test]
    fn hidden_network_settings_round_trip() {
        let mut p = Project::new("p");
        let sid = p.zones[0].snapshots[0].id;
        let (a, b) = (MacAddress([2, 0, 0, 0, 0, 1]), MacAddress([2, 0, 0, 0, 0, 2]));
        let c = p.canvas.snapshot_mut(sid);
        c.hidden_excluded.insert(a);
        c.hidden_labels.insert(a, "Camera".into());
        c.hidden_labels.insert(b, "Printer".into());
        let mut store = Store::in_memory().unwrap();
        store.save(&p).unwrap();
        let loaded = store.load().unwrap();
        let l = loaded.canvas.snapshot(sid).unwrap();
        assert_eq!(l.hidden_excluded.iter().copied().collect::<Vec<_>>(), [a]);
        assert_eq!(l.hidden_labels.get(&b).map(String::as_str), Some("Printer"));
        assert_eq!(loaded, p);
    }

    #[test]
    fn provenance_round_trips_in_order() {
        let mut p = Project::new("p");
        let sid = p.zones[0].snapshots[0].id;
        p.canvas.snapshot_mut(sid).provenance = vec![
            Provenance { source_name: "B".into(), points_in: 5, points_kept: 4 },
            Provenance { source_name: "A".into(), points_in: 7, points_kept: 7 },
        ];
        let mut store = Store::in_memory().unwrap();
        store.save(&p).unwrap();
        assert_eq!(store.load().unwrap(), p);
    }

    #[test]
    fn manual_ap_positions_round_trip() {
        let mut p = Project::new("p");
        let sid = p.zones[0].snapshots[0].id;
        let a = MacAddress([2, 0, 0, 0, 0, 9]);
        p.canvas.snapshot_mut(sid).manual_aps.insert(a, (12.5, -3.25));
        let mut store = Store::in_memory().unwrap();
        store.save(&p).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.canvas.snapshot(sid).unwrap().manual_aps.get(&a), Some(&(12.5, -3.25)));
        assert_eq!(loaded, p);
    }

    #[test]
    fn sampling_settings_round_trip() {
        let mut p = Project::new("p");
        let zid = p.zones[0].id;
        let z = p.canvas.zone_mut(zid);
        z.radius = Some(RadiusPreset::Large);
        z.samples_per_point = 4;
        let mut store = Store::in_memory().unwrap();
        store.save(&p).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.canvas.zone(zid).unwrap().radius, Some(RadiusPreset::Large));
        assert_eq!(loaded.canvas.zone(zid).unwrap().samples_per_point, 4);
        assert_eq!(loaded, p);
    }

    #[test]
    fn drawing_round_trips_with_shape_order_and_styles() {
        use openspot_model::{Drawing, FontSize, LineThickness, Shape, ShapeKind};
        let mut p = Project::new("p");
        p.zones.push(openspot_model::Zone::new("second, empty"));
        let zid = p.zones[0].id;
        let mut d = Drawing::new(1600, 900);
        d.shapes.push(Shape::new(ShapeKind::Polyline { points: vec![(1.0, 2.0), (3.0, 4.0), (5.5, 6.5)] }, LineThickness::Thick, FontSize::Small));
        d.shapes.push(Shape::new(ShapeKind::Rect { min: (10.0, 20.0), max: (30.0, 40.0), filled: true }, LineThickness::Thin, FontSize::Medium));
        d.shapes.push(Shape::new(ShapeKind::Text { at: (7.0, 8.0), text: "Lobby\nfloor 2, \"east\"".into() }, LineThickness::Medium, FontSize::Huge));
        p.canvas.zone_mut(zid).drawing = Some(d.clone());
        let mut store = Store::in_memory().unwrap();
        store.save(&p).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.canvas.zone(zid).unwrap().drawing.as_ref(), Some(&d));
        assert_eq!(loaded, p);
        // Saving again replaces rather than duplicates.
        store.save(&loaded).unwrap();
        assert_eq!(store.load().unwrap(), p);
    }

    #[test]
    fn drawing_shares_a_zone_row_with_other_settings() {
        use openspot_model::Drawing;
        let mut p = Project::new("p");
        let zid = p.zones[0].id;
        let z = p.canvas.zone_mut(zid);
        z.samples_per_point = 5;
        z.drawing = Some(Drawing::new(10, 10));
        let mut store = Store::in_memory().unwrap();
        store.save(&p).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.canvas.zone(zid).unwrap().samples_per_point, 5);
        assert!(loaded.canvas.zone(zid).unwrap().drawing.is_some());
    }

    #[test]
    fn crop_and_gray_out_round_trip() {
        for crop in [
            CropArea::rect((10.0, 20.0), (110.0, 220.0)),
            CropArea::polygon(vec![(0.0, 0.0), (50.0, 5.0), (20.0, 40.0), (1.0, 9.0)]).unwrap(),
        ] {
            let mut p = Project::new("p");
            let zid = p.zones[0].id;
            let z = p.canvas.zone_mut(zid);
            z.crop = Some(crop.clone());
            z.gray_unmeasured = true;
            let mut store = Store::in_memory().unwrap();
            store.save(&p).unwrap();
            let loaded = store.load().unwrap();
            assert_eq!(loaded.canvas.zone(zid).unwrap().crop, Some(crop));
            assert!(loaded.canvas.zone(zid).unwrap().gray_unmeasured);
            assert_eq!(loaded, p);
        }
    }

    #[test]
    fn corrupt_samples_value_is_clamped_not_fatal() {
        let mut p = Project::new("p");
        let zid = p.zones[0].id;
        p.canvas.zone_mut(zid).samples_per_point = 3;
        let mut store = Store::in_memory().unwrap();
        store.save(&p).unwrap();
        store.conn.execute("UPDATE canvas_zone SET samples_per_point = 9999, radius_code = 77", []).unwrap();
        let z = store.load().unwrap().canvas.zone(zid).cloned().unwrap();
        assert_eq!(z.samples_per_point, MAX_SAMPLES_PER_POINT);
        assert_eq!(z.radius, None);
    }
}
