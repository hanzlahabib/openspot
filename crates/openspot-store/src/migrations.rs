//! Ordered schema migrations. Migration `i` takes a database from `user_version == i` to
//! `i + 1`; a fresh file runs all of them. Never edit a released migration: add a new one.
//!
//! Slots 2-6 belong to feature modules in `ext/` (speed, survey, planning, structure, spare) so
//! parallel feature work does not collide.

use crate::ext;

const V1_BASE: &str = "
CREATE TABLE project (id TEXT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE zone (
    id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, name TEXT NOT NULL,
    map_name TEXT, map_width INTEGER, map_height INTEGER, map_data BLOB,
    pixels_per_meter REAL);
CREATE TABLE snapshot (
    id TEXT PRIMARY KEY, zone_id TEXT NOT NULL REFERENCES zone(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, name TEXT NOT NULL);
CREATE TABLE scan_point (
    id TEXT PRIMARY KEY, snapshot_id TEXT NOT NULL REFERENCES snapshot(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, x REAL NOT NULL, y REAL NOT NULL, time INTEGER NOT NULL);
CREATE TABLE measurement (
    id INTEGER PRIMARY KEY, scan_point_id TEXT NOT NULL REFERENCES scan_point(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, bssid BLOB NOT NULL, ssid TEXT NOT NULL, rssi_dbm INTEGER NOT NULL,
    frequency_khz INTEGER NOT NULL, privacy INTEGER NOT NULL, beacon_interval_tu INTEGER NOT NULL,
    ies BLOB NOT NULL);
CREATE INDEX measurement_point ON measurement(scan_point_id);
";

pub(crate) const ALL: &[&str] = &[V1_BASE, ext::speed::SQL, ext::survey::SQL, ext::planning::SQL, ext::structure::SQL, ext::spare::SQL];

/// Position of the structure migration in [`ALL`]; migrations before it are "old" files.
#[cfg(test)]
pub(crate) const STRUCTURE_SLOT: usize = 4;

/// Position of the spare-slot migration (per-project aliases, link info) in [`ALL`].
#[cfg(test)]
pub(crate) const SPARE_SLOT: usize = 5;
