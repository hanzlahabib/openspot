//! Planning mode: level placement, walls, openings, cutouts, custom materials, virtual APs.
//!
//! Enums and polygons are stored as JSON text (their serde form), so adding a variant
//! needs no migration; everything queryable is a plain column.

use rusqlite::{Connection, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};

use openspot_model::{
    AntennaPattern, Band, Cutout, Level, Material, Opening, P2040Params, PerBand, PlanningData, Point, Project, Radio,
    Slab, VirtualAp, Wall,
};

use crate::{StoreError, parse_uuid};

/// DDL for this feature's migration.
pub(crate) const SQL: &str = "
CREATE TABLE plan_level (
    zone_id TEXT PRIMARY KEY REFERENCES zone(id) ON DELETE CASCADE,
    elevation_m REAL NOT NULL, floor_height_m REAL NOT NULL, offset_x REAL NOT NULL, offset_y REAL NOT NULL,
    slab_thickness_m REAL NOT NULL, slab_material TEXT NOT NULL);
CREATE TABLE plan_material (
    zone_id TEXT NOT NULL REFERENCES zone(id) ON DELETE CASCADE, position INTEGER NOT NULL,
    id TEXT NOT NULL, name TEXT NOT NULL, category TEXT NOT NULL, reference_thickness_m REAL,
    abs_24 REAL NOT NULL, abs_5 REAL NOT NULL, abs_6 REAL NOT NULL,
    refl_24 REAL NOT NULL, refl_5 REAL NOT NULL, refl_6 REAL NOT NULL,
    source TEXT NOT NULL, p2040_a REAL, p2040_b REAL, p2040_c REAL, p2040_d REAL,
    PRIMARY KEY (zone_id, position));
CREATE TABLE plan_wall (
    id TEXT PRIMARY KEY, zone_id TEXT NOT NULL REFERENCES zone(id) ON DELETE CASCADE, position INTEGER NOT NULL,
    ax REAL NOT NULL, ay REAL NOT NULL, bx REAL NOT NULL, by REAL NOT NULL,
    thickness_m REAL NOT NULL, kind TEXT NOT NULL, material TEXT NOT NULL);
CREATE TABLE plan_opening (
    id TEXT PRIMARY KEY, wall_id TEXT NOT NULL REFERENCES plan_wall(id) ON DELETE CASCADE, position INTEGER NOT NULL,
    kind TEXT NOT NULL, start REAL NOT NULL, \"end\" REAL NOT NULL, material TEXT NOT NULL);
CREATE TABLE plan_cutout (
    id TEXT PRIMARY KEY, zone_id TEXT NOT NULL REFERENCES zone(id) ON DELETE CASCADE, position INTEGER NOT NULL,
    polygon TEXT NOT NULL);
CREATE TABLE plan_ap (
    id TEXT PRIMARY KEY, zone_id TEXT NOT NULL REFERENCES zone(id) ON DELETE CASCADE, position INTEGER NOT NULL,
    name TEXT NOT NULL, x REAL NOT NULL, y REAL NOT NULL, height_m REAL NOT NULL);
CREATE TABLE plan_radio (
    ap_id TEXT NOT NULL REFERENCES plan_ap(id) ON DELETE CASCADE, position INTEGER NOT NULL,
    band TEXT NOT NULL, channel INTEGER NOT NULL, width_mhz INTEGER NOT NULL, tx_power_dbm REAL NOT NULL,
    antenna_gain_dbi REAL NOT NULL, enabled INTEGER NOT NULL, pattern TEXT NOT NULL,
    PRIMARY KEY (ap_id, position));
";

fn to_text<T: Serialize>(v: &T) -> Result<String, StoreError> {
    serde_json::to_string(v).map_err(|_| StoreError::Corrupt("planning value cannot be encoded"))
}

fn from_text<T: DeserializeOwned>(s: &str) -> Result<T, StoreError> {
    serde_json::from_str(s).map_err(|_| StoreError::Corrupt("planning value is not understood"))
}

/// Writes this feature's data inside the save transaction (after the core tables).
pub(crate) fn save(tx: &Transaction<'_>, project: &Project) -> Result<(), StoreError> {
    for zone in &project.zones {
        let zid = zone.id.to_string();
        let p = &zone.planning;
        tx.execute(
            "INSERT INTO plan_level VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![zid, p.level.elevation_m, p.level.floor_height_m, p.level.offset.x, p.level.offset.y, p.level.slab.thickness_m, p.level.slab.material],
        )?;
        for (i, m) in p.materials.iter().enumerate() {
            let (a, b, c, d) = m.p2040.map_or((None, None, None, None), |q| (Some(q.a), Some(q.b), Some(q.c), Some(q.d)));
            tx.execute(
                "INSERT INTO plan_material VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                params![
                    zid, i, m.id, m.name, to_text(&m.category)?, m.reference_thickness_m,
                    m.absorption_db.ghz2_4, m.absorption_db.ghz5, m.absorption_db.ghz6,
                    m.reflection.ghz2_4, m.reflection.ghz5, m.reflection.ghz6, m.source, a, b, c, d
                ],
            )?;
        }
        for (i, w) in p.walls.iter().enumerate() {
            tx.execute(
                "INSERT INTO plan_wall VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![w.id.to_string(), zid, i, w.a.x, w.a.y, w.b.x, w.b.y, w.thickness_m, to_text(&w.kind)?, w.material],
            )?;
            for (j, o) in w.openings.iter().enumerate() {
                tx.execute(
                    "INSERT INTO plan_opening VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![o.id.to_string(), w.id.to_string(), j, to_text(&o.kind)?, o.start, o.end, o.material],
                )?;
            }
        }
        for (i, c) in p.cutouts.iter().enumerate() {
            tx.execute("INSERT INTO plan_cutout VALUES (?1,?2,?3,?4)", params![c.id.to_string(), zid, i, to_text(&c.polygon)?])?;
        }
        for (i, ap) in p.access_points.iter().enumerate() {
            tx.execute(
                "INSERT INTO plan_ap VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![ap.id.to_string(), zid, i, ap.name, ap.position.x, ap.position.y, ap.height_m],
            )?;
            for (j, r) in ap.radios.iter().enumerate() {
                tx.execute(
                    "INSERT INTO plan_radio VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        ap.id.to_string(), j, to_text(&r.band)?, r.channel, r.width_mhz, r.tx_power_dbm,
                        r.antenna_gain_dbi, r.enabled, to_text(&r.pattern)?
                    ],
                )?;
            }
        }
    }
    Ok(())
}

/// Reads this feature's data into `project` (after the core tables are loaded).
pub(crate) fn load(conn: &Connection, project: &mut Project) -> Result<(), StoreError> {
    for zone in &mut project.zones {
        zone.planning = load_zone(conn, &zone.id.to_string())?;
    }
    Ok(())
}

fn load_zone(conn: &Connection, zid: &str) -> Result<PlanningData, StoreError> {
    let mut data = PlanningData::default();
    let level = conn.query_row(
        "SELECT elevation_m, floor_height_m, offset_x, offset_y, slab_thickness_m, slab_material FROM plan_level WHERE zone_id = ?1",
        [zid],
        |r| {
            Ok(Level {
                elevation_m: r.get(0)?,
                floor_height_m: r.get(1)?,
                offset: Point::new(r.get(2)?, r.get(3)?),
                slab: Slab { thickness_m: r.get(4)?, material: r.get(5)? },
            })
        },
    );
    match level {
        Ok(l) => data.level = l,
        // Files saved before this feature existed have no row: keep the default level.
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        Err(e) => return Err(e.into()),
    }

    let mut stmt = conn.prepare(
        "SELECT id, name, category, reference_thickness_m, abs_24, abs_5, abs_6, refl_24, refl_5, refl_6, source,
                p2040_a, p2040_b, p2040_c, p2040_d FROM plan_material WHERE zone_id = ?1 ORDER BY position",
    )?;
    let rows = stmt
        .query_map([zid], |r| {
            Ok((
                r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<f64>>(3)?,
                PerBand { ghz2_4: r.get(4)?, ghz5: r.get(5)?, ghz6: r.get(6)? },
                PerBand { ghz2_4: r.get(7)?, ghz5: r.get(8)?, ghz6: r.get(9)? },
                r.get::<_, String>(10)?,
                [r.get::<_, Option<f64>>(11)?, r.get(12)?, r.get(13)?, r.get(14)?],
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, name, category, reference_thickness_m, absorption_db, reflection, source, p) in rows {
        let p2040 = match p {
            [Some(a), Some(b), Some(c), Some(d)] => Some(P2040Params { a, b, c, d }),
            _ => None,
        };
        data.materials.push(Material { id, name, category: from_text(&category)?, reference_thickness_m, absorption_db, reflection, source, p2040 });
    }

    let mut stmt = conn.prepare(
        "SELECT id, ax, ay, bx, by, thickness_m, kind, material FROM plan_wall WHERE zone_id = ?1 ORDER BY position",
    )?;
    let rows = stmt
        .query_map([zid], |r| {
            Ok((r.get::<_, String>(0)?, [r.get::<_, f64>(1)?, r.get(2)?, r.get(3)?, r.get(4)?], r.get::<_, f64>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, [ax, ay, bx, by], thickness_m, kind, material) in rows {
        data.walls.push(Wall {
            openings: load_openings(conn, &id)?,
            id: parse_uuid(&id)?,
            a: Point::new(ax, ay),
            b: Point::new(bx, by),
            thickness_m,
            kind: from_text(&kind)?,
            material,
        });
    }

    let mut stmt = conn.prepare("SELECT id, polygon FROM plan_cutout WHERE zone_id = ?1 ORDER BY position")?;
    let rows = stmt.query_map([zid], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    for (id, polygon) in rows {
        data.cutouts.push(Cutout { id: parse_uuid(&id)?, polygon: from_text(&polygon)? });
    }

    let mut stmt = conn.prepare("SELECT id, name, x, y, height_m FROM plan_ap WHERE zone_id = ?1 ORDER BY position")?;
    let rows = stmt
        .query_map([zid], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?, r.get::<_, f64>(4)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, name, x, y, height_m) in rows {
        data.access_points.push(VirtualAp { radios: load_radios(conn, &id)?, id: parse_uuid(&id)?, name, position: Point::new(x, y), height_m });
    }
    Ok(data)
}

fn load_openings(conn: &Connection, wall_id: &str) -> Result<Vec<Opening>, StoreError> {
    let mut stmt = conn.prepare("SELECT id, kind, start, \"end\", material FROM plan_opening WHERE wall_id = ?1 ORDER BY position")?;
    let rows = stmt
        .query_map([wall_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?, r.get::<_, String>(4)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, kind, start, end, material)| Ok(Opening { id: parse_uuid(&id)?, kind: from_text(&kind)?, start, end, material }))
        .collect()
}

fn load_radios(conn: &Connection, ap_id: &str) -> Result<Vec<Radio>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT band, channel, width_mhz, tx_power_dbm, antenna_gain_dbi, enabled, pattern
         FROM plan_radio WHERE ap_id = ?1 ORDER BY position",
    )?;
    let rows = stmt
        .query_map([ap_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u16>(1)?, r.get::<_, u16>(2)?, r.get::<_, f64>(3)?, r.get::<_, f64>(4)?, r.get::<_, bool>(5)?, r.get::<_, String>(6)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(band, channel, width_mhz, tx_power_dbm, antenna_gain_dbi, enabled, pattern)| {
            Ok(Radio {
                band: from_text::<Band>(&band)?,
                channel,
                width_mhz,
                tx_power_dbm,
                antenna_gain_dbi,
                pattern: from_text::<AntennaPattern>(&pattern)?,
                enabled,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use openspot_model::{
        AntennaTable, MaterialCategory, OpeningKind, WallKind, Zone,
    };

    use super::*;
    use crate::Store;

    fn rich_zone() -> Zone {
        let mut z = Zone::new("Level 2");
        let p = &mut z.planning;
        p.level = Level { elevation_m: 3.2, floor_height_m: 3.5, offset: Point::new(-1.5, 0.25), slab: Slab { thickness_m: 0.3, material: "my-slab".into() } };
        p.materials.push(Material::custom("my-slab", "Slab", MaterialCategory::Slab, 18.5, 0.4));
        p.materials.push(Material::from_p2040(
            "brick2", "Brick", MaterialCategory::Wall, P2040Params { a: 3.91, b: 0.0, c: 0.0238, d: 0.16 }, 0.12,
        ));
        let mut wall = Wall::new(Point::new(0.0, 0.0), Point::new(10.0, 0.5), WallKind::Wall, "brick2");
        wall.thickness_m = 0.25;
        wall.openings.push(Opening { id: openspot_model::Uuid::new_v4(), kind: OpeningKind::Door, start: 0.4, end: 0.5, material: "wood-door".into() });
        p.walls.push(wall);
        p.walls.push(Wall::new(Point::new(1.0, 1.0), Point::new(1.0, 5.0), WallKind::Shelf, "chipboard-shelf"));
        p.cutouts.push(Cutout { id: openspot_model::Uuid::new_v4(), polygon: vec![Point::new(0.0, 0.0), Point::new(2.0, 0.0), Point::new(1.0, 2.0)] });
        let mut dir = Radio::new(Band::Ghz5, 36, 80, 17.0);
        dir.pattern = AntennaPattern::Directional { azimuth_deg: 90.0, h_beamwidth_deg: 65.0, v_beamwidth_deg: 30.0, tilt_deg: 10.0, front_to_back_db: 25.0 };
        let mut table = Radio::new(Band::Ghz6, 5, 160, 20.0);
        table.enabled = false;
        table.pattern = AntennaPattern::Table {
            azimuth_deg: 0.0,
            tilt_deg: 0.0,
            table: AntennaTable { name: "t".into(), azimuth: vec![(-180.0, -20.0), (0.0, 0.0), (180.0, -20.0)], elevation: vec![(-90.0, -9.0), (90.0, -9.0)] },
        };
        let mut ap = VirtualAp::new("AP-1", Point::new(4.0, 2.0), vec![Radio::new(Band::Ghz2_4, 6, 20, 14.0), dir, table]);
        ap.height_m = 2.8;
        p.access_points.push(ap);
        p.access_points.push(VirtualAp::new("no radios", Point::new(0.0, 0.0), vec![]));
        z
    }

    #[test]
    fn planning_data_round_trips_including_empty_fields() {
        let mut project = Project::new("plan");
        project.zones.push(rich_zone());
        // zone 0 stays at defaults: empty vectors, no custom material, no p2040.
        let mut store = Store::in_memory().unwrap();
        store.save(&project).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded, project);
        assert!(loaded.zones[0].planning.is_empty());
        assert_eq!(loaded.zones[1].planning.walls[0].openings.len(), 1);
        assert!(loaded.zones[1].planning.materials[0].p2040.is_none());
        assert!(loaded.zones[1].planning.materials[1].p2040.is_some());
    }

    #[test]
    fn resaving_replaces_planning_rows() {
        let mut project = Project::new("plan");
        project.zones[0] = rich_zone();
        let mut store = Store::in_memory().unwrap();
        store.save(&project).unwrap();
        project.zones[0].planning.walls.clear();
        project.zones[0].planning.access_points.truncate(1);
        store.save(&project).unwrap();
        assert_eq!(store.load().unwrap(), project);
    }
}
