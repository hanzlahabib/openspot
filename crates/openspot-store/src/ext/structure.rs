//! Project structure: buildings, floor order, explorer metadata (created/changed times).

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use openspot_model::{Building, Floor, FloorAlignment, Project, ProjectSummary, Structure};

use crate::{Store, StoreError, parse_uuid};

/// DDL for this feature's migration. Both tables hang off `project`/`zone`, so the
/// `DELETE FROM project` at the start of every save clears them too.
pub(crate) const SQL: &str = "
CREATE TABLE structure_meta (
    project_id TEXT PRIMARY KEY REFERENCES project(id) ON DELETE CASCADE,
    created INTEGER NOT NULL, changed INTEGER NOT NULL);
CREATE TABLE building (
    id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, name TEXT NOT NULL);
CREATE TABLE building_floor (
    building_id TEXT NOT NULL REFERENCES building(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    zone_id TEXT NOT NULL REFERENCES zone(id) ON DELETE CASCADE,
    label TEXT NOT NULL, height_m REAL NOT NULL,
    dx_m REAL NOT NULL, dy_m REAL NOT NULL, rotation_rad REAL NOT NULL);
";

/// Writes this feature's data inside the save transaction (after the core tables).
pub(crate) fn save(tx: &Transaction<'_>, project: &Project) -> Result<(), StoreError> {
    let pid = project.id.to_string();
    let s = &project.structure;
    tx.execute("INSERT INTO structure_meta (project_id, created, changed) VALUES (?1, ?2, ?3)", params![pid, s.created, s.changed])?;
    for (bi, b) in s.buildings.iter().enumerate() {
        tx.execute("INSERT INTO building (id, project_id, position, name) VALUES (?1, ?2, ?3, ?4)", params![b.id.to_string(), pid, bi, b.name])?;
        for (fi, f) in b.floors.iter().enumerate() {
            tx.execute(
                "INSERT INTO building_floor (building_id, position, zone_id, label, height_m, dx_m, dy_m, rotation_rad)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![b.id.to_string(), fi, f.zone.to_string(), f.label, f.height_m, f.alignment.dx_m, f.alignment.dy_m, f.alignment.rotation_rad],
            )?;
        }
    }
    Ok(())
}

/// Reads this feature's data into `project` (after the core tables are loaded).
/// A project saved before this feature has no rows and keeps the default (no buildings,
/// unknown timestamps).
pub(crate) fn load(conn: &Connection, project: &mut Project) -> Result<(), StoreError> {
    let mut structure = Structure::default();
    if let Some((created, changed)) = conn
        .query_row("SELECT created, changed FROM structure_meta", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
    {
        structure.created = created;
        structure.changed = changed;
    }
    let mut stmt = conn.prepare("SELECT id, name FROM building ORDER BY position")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    for (id, name) in rows {
        let mut fstmt = conn.prepare(
            "SELECT zone_id, label, height_m, dx_m, dy_m, rotation_rad FROM building_floor
             WHERE building_id = ?1 ORDER BY position",
        )?;
        let floors = fstmt
            .query_map([&id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?, r.get::<_, f64>(4)?, r.get::<_, f64>(5)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let floors = floors
            .into_iter()
            .map(|(zone, label, height_m, dx_m, dy_m, rotation_rad)| {
                Ok(Floor { zone: parse_uuid(&zone)?, label, height_m, alignment: FloorAlignment { dx_m, dy_m, rotation_rad } })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        structure.buildings.push(Building { id: parse_uuid(&id)?, name, floors });
    }
    project.structure = structure;
    Ok(())
}

impl Store {
    /// Name, timestamps and zone count without loading any measurements, for the explorer.
    pub fn summary(&self) -> Result<ProjectSummary, StoreError> {
        let name: String = self.conn.query_row("SELECT name FROM project", [], |r| r.get(0)).optional()?.ok_or(StoreError::Empty)?;
        let zone_count: i64 = self.conn.query_row("SELECT COUNT(*) FROM zone", [], |r| r.get(0))?;
        let (created, changed) = self
            .conn
            .query_row("SELECT created, changed FROM structure_meta", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .unwrap_or((0, 0));
        Ok(ProjectSummary { name, created, changed, zone_count: zone_count as usize })
    }
}
