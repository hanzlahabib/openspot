//! Uniform-grid spatial index for [`crate::nearest`] (categorical maps).
//!
//! Brute force costs cells x samples (about 1.4 s for 10 000 points on a 500 x 375 grid). The
//! index buckets samples into squares at least as wide as the query radius, so a query only
//! has to look at the 3 x 3 buckets around it. The answer is *identical* to the brute-force
//! scan, ties included: the sample with the smallest squared distance wins and, among equal
//! distances, the one that comes first in the input, exactly as `Iterator::min_by` over the
//! samples in order behaves.

use std::collections::HashMap;

use crate::Sample;

/// Smallest bucket edge in pixels. A zero or tiny radius would otherwise create a bucket per
/// pixel (or divide by zero); a larger bucket only adds a few candidates per query.
const MIN_BUCKET_PX: f64 = 1.0;

type BucketKey = (i64, i64);

pub(crate) struct SampleIndex<'a> {
    samples: &'a [Sample],
    /// Bucket edge length, never smaller than the query radius.
    edge: f64,
    /// Sample indices per bucket, in input order.
    buckets: HashMap<BucketKey, Vec<u32>>,
}

impl<'a> SampleIndex<'a> {
    /// Returns `None` when the radius cannot bound a search (not finite): the caller must
    /// then scan every sample.
    pub(crate) fn new(samples: &'a [Sample], radius_px: f64) -> Option<Self> {
        if !radius_px.is_finite() || u32::try_from(samples.len()).is_err() {
            return None;
        }
        // The scan compares squared distances, so a negative radius behaves like its absolute value.
        let edge = radius_px.abs().max(MIN_BUCKET_PX);
        let mut buckets: HashMap<BucketKey, Vec<u32>> = HashMap::new();
        for (i, s) in samples.iter().enumerate() {
            // A sample with a non-finite coordinate has a NaN or infinite distance to every
            // cell, which never passes the `<= radius^2` test, so leaving it out changes nothing.
            if s.x.is_finite() && s.y.is_finite() {
                buckets.entry(bucket_of(s.x, s.y, edge)).or_default().push(i as u32);
            }
        }
        Some(Self { samples, edge, buckets })
    }

    /// Value of the nearest sample within `r2` (a squared distance) of `(px, py)`.
    pub(crate) fn nearest_value(&self, px: f64, py: f64, r2: f64) -> Option<f64> {
        let (bx, by) = bucket_of(px, py, self.edge);
        let mut best: Option<(f64, u32)> = None;
        for gx in bx.saturating_sub(1)..=bx.saturating_add(1) {
            for gy in by.saturating_sub(1)..=by.saturating_add(1) {
                let Some(bucket) = self.buckets.get(&(gx, gy)) else { continue };
                for &i in bucket {
                    let s = &self.samples[i as usize];
                    // Same expression as the brute-force scan so the distances are bit-identical.
                    let d2 = (s.x - px).powi(2) + (s.y - py).powi(2);
                    if d2 > r2 || d2.is_nan() {
                        continue;
                    }
                    if best.is_none_or(|(bd, bi)| d2.total_cmp(&bd).then(i.cmp(&bi)).is_lt()) {
                        best = Some((d2, i));
                    }
                }
            }
        }
        best.map(|(_, i)| self.samples[i as usize].value)
    }
}

/// Bucket containing `(x, y)`. Float-to-int casts saturate, so absurd coordinates share the
/// outermost bucket instead of overflowing; that only costs extra candidates.
fn bucket_of(x: f64, y: f64, edge: f64) -> BucketKey {
    ((x / edge).floor() as i64, (y / edge).floor() as i64)
}
