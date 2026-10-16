//! Spare slot (migration 6), used by the polish features: per-project aliases (I-06) and the
//! wireless link recorded with each measurement point (H-10).

use std::collections::HashMap;

use rusqlite::{Connection, Transaction, params};

use openspot_model::{LinkInfo, Project};
use openspot_wlan::MacAddress;

use crate::StoreError;

/// DDL for this feature's migration. Both tables are created together so the migration never
/// has to change once released.
pub(crate) const SQL: &str = "
CREATE TABLE project_alias (
    project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    bssid BLOB NOT NULL, alias TEXT NOT NULL,
    PRIMARY KEY (project_id, bssid));
CREATE TABLE scan_link (
    scan_point_id TEXT PRIMARY KEY REFERENCES scan_point(id) ON DELETE CASCADE,
    bssid BLOB NOT NULL, tx_rate_mbps REAL NOT NULL, rx_rate_mbps REAL NOT NULL);
";

/// Writes this feature's data inside the save transaction (after the core tables).
pub(crate) fn save(tx: &Transaction<'_>, project: &Project) -> Result<(), StoreError> {
    for (bssid, alias) in &project.aliases.by_bssid {
        tx.execute(
            "INSERT INTO project_alias (project_id, bssid, alias) VALUES (?1, ?2, ?3)",
            params![project.id.to_string(), bssid.0.as_slice(), alias],
        )?;
    }
    let points = project.zones.iter().flat_map(|z| &z.snapshots).flat_map(|s| &s.points);
    for (point, link) in points.filter_map(|p| Some((p, p.link.as_ref()?))) {
        tx.execute(
            "INSERT INTO scan_link (scan_point_id, bssid, tx_rate_mbps, rx_rate_mbps) VALUES (?1, ?2, ?3, ?4)",
            params![point.id.to_string(), link.bssid.0.as_slice(), link.tx_rate_mbps, link.rx_rate_mbps],
        )?;
    }
    Ok(())
}

fn mac(bytes: &[u8], what: &'static str) -> Result<MacAddress, StoreError> {
    Ok(MacAddress(<[u8; 6]>::try_from(bytes).map_err(|_| StoreError::Corrupt(what))?))
}

/// Reads this feature's data into `project` (after the core tables are loaded).
pub(crate) fn load(conn: &Connection, project: &mut Project) -> Result<(), StoreError> {
    let mut stmt = conn.prepare("SELECT bssid, alias FROM project_alias WHERE project_id = ?1 ORDER BY bssid")?;
    let rows = stmt.query_map([project.id.to_string()], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (bssid, alias) = row?;
        project.aliases.by_bssid.insert(mac(&bssid, "alias bssid is not 6 bytes")?, alias);
    }

    let mut stmt = conn.prepare("SELECT scan_point_id, bssid, tx_rate_mbps, rx_rate_mbps FROM scan_link")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?))
    })?;
    let mut links = HashMap::new();
    for row in rows {
        let (point, bssid, tx_rate_mbps, rx_rate_mbps) = row?;
        links.insert(point, LinkInfo { bssid: mac(&bssid, "link bssid is not 6 bytes")?, tx_rate_mbps, rx_rate_mbps });
    }
    if !links.is_empty() {
        let points = project.zones.iter_mut().flat_map(|z| &mut z.snapshots).flat_map(|s| &mut s.points);
        for point in points {
            point.link = links.remove(&point.id.to_string());
        }
    }
    Ok(())
}
