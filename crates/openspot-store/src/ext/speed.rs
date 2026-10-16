//! Active-scan (speed test) results per measurement point.

use std::collections::HashMap;

use rusqlite::{Connection, Transaction, params};

use openspot_model::{Project, SpeedResult};
use openspot_wlan::MacAddress;

use crate::StoreError;

/// DDL for this feature's migration. Every measurement column is nullable: NULL means "not
/// measured", which must stay distinguishable from a measured 0.
pub(crate) const SQL: &str = "
CREATE TABLE speed_result (
    id INTEGER PRIMARY KEY,
    scan_point_id TEXT NOT NULL REFERENCES scan_point(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    ssid TEXT NOT NULL, bssid BLOB,
    http_down_mbps REAL, http_up_mbps REAL,
    iperf_tcp_down_mbps REAL, iperf_tcp_up_mbps REAL,
    iperf_udp_down_mbps REAL, iperf_udp_up_mbps REAL,
    udp_jitter_ms REAL, udp_loss_pct REAL);
CREATE INDEX speed_result_point ON speed_result(scan_point_id);
CREATE TABLE speed_error (
    speed_result_id INTEGER NOT NULL REFERENCES speed_result(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, message TEXT NOT NULL);
";

/// Writes this feature's data inside the save transaction (after the core tables).
pub(crate) fn save(tx: &Transaction<'_>, project: &Project) -> Result<(), StoreError> {
    let points = project.zones.iter().flat_map(|z| &z.snapshots).flat_map(|s| &s.points);
    for point in points {
        for (pos, r) in point.speed.iter().enumerate() {
            tx.execute(
                "INSERT INTO speed_result (scan_point_id, position, ssid, bssid, http_down_mbps, http_up_mbps,
                     iperf_tcp_down_mbps, iperf_tcp_up_mbps, iperf_udp_down_mbps, iperf_udp_up_mbps,
                     udp_jitter_ms, udp_loss_pct)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    point.id.to_string(),
                    pos,
                    r.ssid,
                    r.bssid.map(|b| b.0.to_vec()),
                    r.http_down_mbps,
                    r.http_up_mbps,
                    r.iperf_tcp_down_mbps,
                    r.iperf_tcp_up_mbps,
                    r.iperf_udp_down_mbps,
                    r.iperf_udp_up_mbps,
                    r.udp_jitter_ms,
                    r.udp_loss_pct,
                ],
            )?;
            let row_id = tx.last_insert_rowid();
            for (epos, message) in r.errors.iter().enumerate() {
                tx.execute(
                    "INSERT INTO speed_error (speed_result_id, position, message) VALUES (?1, ?2, ?3)",
                    params![row_id, epos, message],
                )?;
            }
        }
    }
    Ok(())
}

/// Reads this feature's data into `project` (after the core tables are loaded).
pub(crate) fn load(conn: &Connection, project: &mut Project) -> Result<(), StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, scan_point_id, ssid, bssid, http_down_mbps, http_up_mbps, iperf_tcp_down_mbps,
                iperf_tcp_up_mbps, iperf_udp_down_mbps, iperf_udp_up_mbps, udp_jitter_ms, udp_loss_pct
         FROM speed_result ORDER BY scan_point_id, position",
    )?;
    let mut errors_stmt =
        conn.prepare("SELECT message FROM speed_error WHERE speed_result_id = ?1 ORDER BY position")?;
    let mut by_point: HashMap<String, Vec<SpeedResult>> = HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let bssid = match r.get::<_, Option<Vec<u8>>>(3)? {
            None => None,
            Some(b) => Some(MacAddress(
                <[u8; 6]>::try_from(b.as_slice()).map_err(|_| StoreError::Corrupt("speed bssid is not 6 bytes"))?,
            )),
        };
        let errors = errors_stmt
            .query_map([r.get::<_, i64>(0)?], |e| e.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        by_point.entry(r.get(1)?).or_default().push(SpeedResult {
            ssid: r.get(2)?,
            bssid,
            http_down_mbps: r.get(4)?,
            http_up_mbps: r.get(5)?,
            iperf_tcp_down_mbps: r.get(6)?,
            iperf_tcp_up_mbps: r.get(7)?,
            iperf_udp_down_mbps: r.get(8)?,
            iperf_udp_up_mbps: r.get(9)?,
            udp_jitter_ms: r.get(10)?,
            udp_loss_pct: r.get(11)?,
            errors,
        });
    }
    let points = project.zones.iter_mut().flat_map(|z| &mut z.snapshots).flat_map(|s| &mut s.points);
    for point in points {
        if let Some(results) = by_point.remove(&point.id.to_string()) {
            point.speed = results;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use openspot_model::ScanPoint;

    use crate::Store;

    use super::*;

    #[test]
    fn speed_results_round_trip_including_none_and_zero() {
        let mut project = Project::new("p");
        let full = SpeedResult {
            ssid: "Home \"WiFi\"".into(),
            bssid: Some(MacAddress([2, 0, 0, 0, 0, 9])),
            http_down_mbps: Some(123.5),
            http_up_mbps: Some(45.25),
            iperf_tcp_down_mbps: Some(300.0),
            iperf_tcp_up_mbps: Some(280.5),
            iperf_udp_down_mbps: Some(50.0),
            iperf_udp_up_mbps: Some(49.5),
            udp_jitter_ms: Some(0.0), // a measured zero must survive, not become None
            udp_loss_pct: Some(1.25),
            errors: vec!["first".into(), "second".into()],
        };
        let sparse = SpeedResult { errors: vec!["iperf3 not found".into()], ..SpeedResult::new("Guest") };
        let mut a = ScanPoint::new(1.0, 2.0, 3, vec![]);
        a.speed = vec![full, sparse];
        let b = ScanPoint::new(4.0, 5.0, 6, vec![]); // no results at all
        project.zones[0].snapshots[0].points = vec![a, b];

        let mut store = Store::in_memory().unwrap();
        store.save(&project).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded, project);
        let pts = &loaded.zones[0].snapshots[0].points;
        assert_eq!(pts[0].speed[1].http_down_mbps, None);
        assert_eq!(pts[0].speed[0].udp_jitter_ms, Some(0.0));
        assert!(pts[1].speed.is_empty());
    }

    #[test]
    fn resaving_does_not_duplicate_results() {
        let mut project = Project::new("p");
        let mut pt = ScanPoint::new(0.0, 0.0, 0, vec![]);
        pt.speed = vec![SpeedResult { http_up_mbps: Some(1.0), ..SpeedResult::new("n") }];
        project.zones[0].snapshots[0].points.push(pt);
        let mut store = Store::in_memory().unwrap();
        store.save(&project).unwrap();
        store.save(&project).unwrap();
        assert_eq!(store.load().unwrap().zones[0].snapshots[0].points[0].speed.len(), 1);
    }
}
