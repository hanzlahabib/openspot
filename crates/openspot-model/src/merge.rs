//! Merging several snapshots of one zone into a new one.

use crate::{ScanPoint, Snapshot, Uuid};

/// Two points within this many seconds of the same bucket boundary grid count as taken in
/// the same session. An hour separates "one walk-through" from "came back later", where a
/// repeated spot is a genuinely new measurement that should be kept.
pub const DEFAULT_TIME_BUCKET_SECS: i64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MergeOptions {
    /// Points closer than this (map pixels) are duplicates when in the same time bucket.
    pub epsilon_px: f64,
    pub time_bucket_secs: i64,
}

impl MergeOptions {
    pub fn new(epsilon_px: f64) -> Self {
        Self { epsilon_px, time_bucket_secs: DEFAULT_TIME_BUCKET_SECS }
    }
}

/// Where merged points came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub source_name: String,
    /// Points the source snapshot had.
    pub points_in: usize,
    /// Of those, how many ended up in the merged snapshot.
    pub points_kept: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub snapshot: Snapshot,
    pub provenance: Vec<Provenance>,
}

impl Merged {
    pub fn duplicates_removed(&self) -> usize {
        self.provenance.iter().map(|p| p.points_in - p.points_kept).sum()
    }
}

fn bucket(time: i64, size: i64) -> i64 {
    time.div_euclid(size.max(1))
}

/// Concatenates the points of `sources` (in the given order) into a new snapshot named
/// `name`. A point is dropped when an earlier kept point is within `epsilon_px` and in the
/// same time bucket, so the first source wins. Kept points get fresh ids (point ids are
/// unique across the project) and the sources are not modified.
pub fn merge_snapshots(sources: &[&Snapshot], name: impl Into<String>, opts: MergeOptions) -> Merged {
    let mut merged = Snapshot::new(name);
    let mut provenance = Vec::with_capacity(sources.len());
    let eps2 = opts.epsilon_px.max(0.0).powi(2);
    for src in sources {
        let mut kept = 0;
        for p in &src.points {
            let b = bucket(p.time, opts.time_bucket_secs);
            let duplicate = merged
                .points
                .iter()
                .any(|k| bucket(k.time, opts.time_bucket_secs) == b && (k.x - p.x).powi(2) + (k.y - p.y).powi(2) < eps2);
            if !duplicate {
                merged.points.push(ScanPoint { id: Uuid::new_v4(), ..p.clone() });
                kept += 1;
            }
        }
        provenance.push(Provenance { source_name: src.name.clone(), points_in: src.points.len(), points_kept: kept });
    }
    Merged { snapshot: merged, provenance }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(name: &str, pts: &[(f64, f64, i64)]) -> Snapshot {
        let mut s = Snapshot::new(name);
        for &(x, y, t) in pts {
            s.points.push(ScanPoint::new(x, y, t, vec![]));
        }
        s
    }

    #[test]
    fn concatenates_in_source_order_with_fresh_ids() {
        let a = snap("A", &[(0.0, 0.0, 100), (50.0, 0.0, 110)]);
        let b = snap("B", &[(0.0, 50.0, 120)]);
        let m = merge_snapshots(&[&a, &b], "AB", MergeOptions::new(5.0));
        let pos: Vec<_> = m.snapshot.points.iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(pos, [(0.0, 0.0), (50.0, 0.0), (0.0, 50.0)]);
        assert_eq!(m.snapshot.name, "AB");
        let all_ids: std::collections::BTreeSet<_> =
            a.points.iter().chain(&b.points).chain(&m.snapshot.points).map(|p| p.id).collect();
        assert_eq!(all_ids.len(), 6, "merged points do not reuse source ids");
        assert_eq!(m.duplicates_removed(), 0);
    }

    #[test]
    fn nearby_points_in_the_same_bucket_are_deduplicated_first_wins() {
        let a = snap("A", &[(10.0, 10.0, 100)]);
        let b = snap("B", &[(12.0, 11.0, 200), (40.0, 40.0, 200)]);
        let m = merge_snapshots(&[&a, &b], "m", MergeOptions::new(5.0));
        assert_eq!(m.snapshot.points.len(), 2);
        assert_eq!((m.snapshot.points[0].x, m.snapshot.points[0].time), (10.0, 100));
        assert_eq!(m.provenance[0], Provenance { source_name: "A".into(), points_in: 1, points_kept: 1 });
        assert_eq!(m.provenance[1], Provenance { source_name: "B".into(), points_in: 2, points_kept: 1 });
        assert_eq!(m.duplicates_removed(), 1);
    }

    #[test]
    fn same_place_in_a_different_time_bucket_is_kept() {
        let a = snap("A", &[(10.0, 10.0, 100)]);
        let b = snap("B", &[(10.0, 10.0, 100 + DEFAULT_TIME_BUCKET_SECS)]);
        let m = merge_snapshots(&[&a, &b], "m", MergeOptions::new(5.0));
        assert_eq!(m.snapshot.points.len(), 2);
    }

    #[test]
    fn distance_threshold_is_strict_and_boundaries_are_exact() {
        let a = snap("A", &[(0.0, 0.0, 0)]);
        let exactly = snap("B", &[(5.0, 0.0, 0)]);
        let inside = snap("C", &[(4.99, 0.0, 0)]);
        assert_eq!(merge_snapshots(&[&a, &exactly], "m", MergeOptions::new(5.0)).snapshot.points.len(), 2);
        assert_eq!(merge_snapshots(&[&a, &inside], "m", MergeOptions::new(5.0)).snapshot.points.len(), 1);
        // Times either side of a bucket boundary are different buckets.
        let late = snap("D", &[(0.0, 0.0, DEFAULT_TIME_BUCKET_SECS)]);
        let early = snap("E", &[(0.0, 0.0, DEFAULT_TIME_BUCKET_SECS - 1)]);
        assert_eq!(merge_snapshots(&[&early, &late], "m", MergeOptions::new(5.0)).snapshot.points.len(), 2);
    }

    #[test]
    fn negative_times_bucket_consistently() {
        let a = snap("A", &[(0.0, 0.0, -1)]);
        let b = snap("B", &[(0.0, 0.0, -3600)]);
        let c = snap("C", &[(0.0, 0.0, 0)]);
        // -1 and -3600 share bucket -1; 0 is bucket 0.
        assert_eq!(merge_snapshots(&[&a, &b], "m", MergeOptions::new(1.0)).snapshot.points.len(), 1);
        assert_eq!(merge_snapshots(&[&a, &c], "m", MergeOptions::new(1.0)).snapshot.points.len(), 2);
    }

    #[test]
    fn sources_are_untouched_and_empty_input_is_empty() {
        let a = snap("A", &[(1.0, 1.0, 0), (1.0, 1.0, 0)]);
        let before = a.clone();
        let m = merge_snapshots(&[&a], "m", MergeOptions::new(5.0));
        assert_eq!(a, before);
        assert_eq!(m.snapshot.points.len(), 1, "duplicates inside one source collapse too");
        assert!(merge_snapshots(&[], "m", MergeOptions::new(5.0)).snapshot.points.is_empty());
    }
}
