//! Merging snapshots of the current zone (V-14).

use openspot_model::{MergeOptions, SnapshotCanvas, merge_snapshots};
use thiserror::Error;

use super::Survey;
use super::history::Edit;

/// Snapshots closer than this in real life, and in the same time bucket, are the same
/// measurement spot: about one step, below the spacing people walk survey points at.
pub const MERGE_EPSILON_M: f64 = 0.5;
/// Merge distance in pixels when the zone has no scale: a few pixels, i.e. a mis-click.
pub const MERGE_FALLBACK_EPSILON_PX: f64 = 6.0;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MergeError {
    #[error("select at least two snapshots to merge")]
    TooFew,
    #[error("a selected snapshot no longer exists")]
    Missing,
}

impl Survey {
    /// Where the current snapshot's points came from, if it was made by merging.
    pub fn snapshot_provenance(&self) -> &[openspot_model::Provenance] {
        self.project.canvas.snapshot(self.snapshot().id).map_or(&[], |c| c.provenance.as_slice())
    }

    pub fn merge_epsilon_px(&self) -> f64 {
        self.zone().scale.map_or(MERGE_FALLBACK_EPSILON_PX, |s| s.to_pixels(MERGE_EPSILON_M))
    }

    /// Merges the snapshots at `indices` (in zone order) into a new snapshot appended to the
    /// zone and selects it. The sources are kept. Returns the new snapshot's index.
    pub fn merge_selected_snapshots(&mut self, indices: &[usize]) -> Result<usize, MergeError> {
        let mut sorted: Vec<usize> = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() < 2 {
            return Err(MergeError::TooFew);
        }
        let zone = self.zone();
        let sources = sorted.iter().map(|&i| zone.snapshots.get(i)).collect::<Option<Vec<_>>>().ok_or(MergeError::Missing)?;
        let name = format!("Merged: {}", sources.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(" + "));
        let merged = merge_snapshots(&sources, name, MergeOptions::new(self.merge_epsilon_px()));
        let canvas = SnapshotCanvas { provenance: merged.provenance, ..SnapshotCanvas::default() };
        let count = merged.snapshot.points.len();
        let snapshot = merged.snapshot;

        let zone_index = self.zone;
        let index = self.zone().snapshots.len();
        self.history.record(Edit::SnapshotInserted { zone: zone_index, index, snapshot: snapshot.clone(), canvas: canvas.clone() });
        self.insert_snapshot_at(zone_index, index, snapshot, canvas);
        self.touch();
        self.message = Some(format!("Merged into a new snapshot with {count} points"));
        Ok(index)
    }
}
