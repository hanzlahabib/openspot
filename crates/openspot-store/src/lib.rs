//! Single-file SQLite store for one [`Project`].
//!
//! Raw information elements are persisted, and access points are re-parsed on load,
//! so parser improvements apply to old surveys without re-measuring.

#![forbid(unsafe_code)]

mod ext;
mod migrations;

use std::path::Path;

use openspot_model::{MapImage, Project, ScanPoint, Scale, Snapshot, Uuid, Zone};
use openspot_wlan::{AccessPoint, MacAddress};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use thiserror::Error;

/// Current schema version: the number of migrations in [`migrations::ALL`].
const SCHEMA_VERSION: i32 = migrations::ALL.len() as i32;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("project file is version {0}, newer than this build supports ({SCHEMA_VERSION})")]
    UnsupportedVersion(i32),
    #[error("project file is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("project file contains no project")]
    Empty,
}

#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let version: i32 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if !(0..=SCHEMA_VERSION).contains(&version) {
            return Err(StoreError::UnsupportedVersion(version));
        }
        for (i, sql) in migrations::ALL.iter().enumerate().skip(version as usize) {
            self.conn.execute_batch("BEGIN")?;
            let applied = self.conn.execute_batch(sql).and_then(|()| self.conn.pragma_update(None, "user_version", i as i32 + 1));
            match applied {
                Ok(()) => self.conn.execute_batch("COMMIT")?,
                Err(e) => {
                    // Best effort: the migration error is the one worth reporting.
                    let _ = self.conn.execute_batch("ROLLBACK");
                    return Err(e.into());
                }
            }
        }
        Ok(())
    }

    /// Replaces the stored project with `project`, atomically.
    pub fn save(&mut self, project: &Project) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM project", [])?;
        tx.execute("INSERT INTO project (id, name) VALUES (?1, ?2)", params![project.id.to_string(), project.name])?;
        for (zi, zone) in project.zones.iter().enumerate() {
            save_zone(&tx, project.id, zi, zone)?;
        }
        ext::speed::save(&tx, project)?;
        ext::survey::save(&tx, project)?;
        ext::planning::save(&tx, project)?;
        ext::structure::save(&tx, project)?;
        ext::spare::save(&tx, project)?;
        tx.commit()?;
        Ok(())
    }

    pub fn load(&self) -> Result<Project, StoreError> {
        let (id, name): (String, String) = self
            .conn
            .query_row("SELECT id, name FROM project", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or(StoreError::Empty)?;
        let mut zones_stmt = self.conn.prepare(
            "SELECT id, name, map_name, map_width, map_height, map_data, pixels_per_meter
             FROM zone ORDER BY position",
        )?;
        let zones = zones_stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<u32>>(3)?,
                    r.get::<_, Option<u32>>(4)?,
                    r.get::<_, Option<Vec<u8>>>(5)?,
                    r.get::<_, Option<f64>>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let zones = zones
            .into_iter()
            .map(|(zid, zname, map_name, w, h, data, ppm)| {
                let map = match (map_name, w, h, data) {
                    (Some(original_name), Some(width), Some(height), Some(data)) => {
                        Some(MapImage { original_name, data, width, height })
                    }
                    _ => None,
                };
                Ok(Zone {
                    id: parse_uuid(&zid)?,
                    name: zname,
                    map,
                    scale: ppm.and_then(Scale::new),
                    snapshots: self.load_snapshots(&zid)?,
                    planning: Default::default(),
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let mut project = Project { id: parse_uuid(&id)?, name, zones, structure: Default::default(), canvas: Default::default(), aliases: Default::default() };
        ext::speed::load(&self.conn, &mut project)?;
        ext::survey::load(&self.conn, &mut project)?;
        ext::planning::load(&self.conn, &mut project)?;
        ext::structure::load(&self.conn, &mut project)?;
        ext::spare::load(&self.conn, &mut project)?;
        Ok(project)
    }

    fn load_snapshots(&self, zone_id: &str) -> Result<Vec<Snapshot>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id, name FROM snapshot WHERE zone_id = ?1 ORDER BY position")?;
        let rows = stmt
            .query_map([zone_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(sid, name)| Ok(Snapshot { id: parse_uuid(&sid)?, name, points: self.load_points(&sid)? }))
            .collect()
    }

    fn load_points(&self, snapshot_id: &str) -> Result<Vec<ScanPoint>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, x, y, time FROM scan_point WHERE snapshot_id = ?1 ORDER BY position")?;
        let rows = stmt
            .query_map([snapshot_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?, r.get::<_, i64>(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(pid, x, y, time)| {
                Ok(ScanPoint { id: parse_uuid(&pid)?, x, y, time, access_points: self.load_measurements(&pid)?, speed: Vec::new(), link: None })
            })
            .collect()
    }

    fn load_measurements(&self, point_id: &str) -> Result<Vec<AccessPoint>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT bssid, ssid, rssi_dbm, frequency_khz, privacy, beacon_interval_tu, ies
             FROM measurement WHERE scan_point_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([point_id], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i32>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, u16>(5)?,
                r.get::<_, Vec<u8>>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (bssid, ssid, rssi, khz, privacy, beacon, ies) = row?;
            let bssid = MacAddress(<[u8; 6]>::try_from(bssid.as_slice()).map_err(|_| StoreError::Corrupt("bssid is not 6 bytes"))?);
            Ok(AccessPoint::from_beacon(ssid, bssid, rssi, khz, privacy, beacon, ies))
        })
        .collect()
    }
}

pub(crate) fn parse_uuid(s: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(s).map_err(|_| StoreError::Corrupt("invalid id"))
}

fn save_zone(tx: &Transaction<'_>, project_id: Uuid, position: usize, zone: &Zone) -> Result<(), StoreError> {
    let map = zone.map.as_ref();
    tx.execute(
        "INSERT INTO zone (id, project_id, position, name, map_name, map_width, map_height, map_data, pixels_per_meter)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            zone.id.to_string(),
            project_id.to_string(),
            position,
            zone.name,
            map.map(|m| &m.original_name),
            map.map(|m| m.width),
            map.map(|m| m.height),
            map.map(|m| &m.data),
            zone.scale.map(|s| s.pixels_per_meter()),
        ],
    )?;
    for (si, snap) in zone.snapshots.iter().enumerate() {
        tx.execute(
            "INSERT INTO snapshot (id, zone_id, position, name) VALUES (?1, ?2, ?3, ?4)",
            params![snap.id.to_string(), zone.id.to_string(), si, snap.name],
        )?;
        for (pi, point) in snap.points.iter().enumerate() {
            tx.execute(
                "INSERT INTO scan_point (id, snapshot_id, position, x, y, time) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![point.id.to_string(), snap.id.to_string(), pi, point.x, point.y, point.time],
            )?;
            for (mi, ap) in point.access_points.iter().enumerate() {
                tx.execute(
                    "INSERT INTO measurement (scan_point_id, position, bssid, ssid, rssi_dbm, frequency_khz,
                                              privacy, beacon_interval_tu, ies)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        point.id.to_string(),
                        mi,
                        ap.bssid.0.as_slice(),
                        ap.ssid,
                        ap.rssi_dbm,
                        ap.frequency_khz,
                        ap.privacy(),
                        ap.beacon_interval_tu,
                        ap.raw_ies,
                    ],
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
