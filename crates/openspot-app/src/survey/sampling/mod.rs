//! Measurement sampling: how far a point's value extends (V-09) and how several scans at
//! one spot are combined into a single point.

use openspot_model::{MAX_SAMPLES_PER_POINT, RadiusPreset, Scale};
use openspot_wlan::{AccessPoint, MacAddress};

/// Extension beyond the sampled area when no radius preset is chosen, as a fraction of
/// the plan's long side. Keeps the edge of a walked path colored without painting the
/// whole floor from three points.
pub const AUTO_EXTEND_FRACTION: f64 = 0.06;

/// Pixels the heatmap extends beyond measurement points. A preset needs the scale to turn
/// its real-world radius into pixels; without one (or with no preset) the automatic
/// fraction of `long_side_px` applies.
pub fn extend_px(preset: Option<RadiusPreset>, scale: Option<Scale>, long_side_px: f64) -> f64 {
    match (preset, scale) {
        (Some(p), Some(s)) => s.to_pixels(p.meters()),
        _ => long_side_px * AUTO_EXTEND_FRACTION,
    }
}

/// Converts dBm to milliwatts.
fn dbm_to_mw(dbm: f64) -> f64 {
    10f64.powf(dbm / 10.0)
}

fn mw_to_dbm(mw: f64) -> f64 {
    10.0 * mw.log10()
}

/// Combines repeated scans of one spot into one reading per access point.
///
/// RSSI is averaged in the linear power domain (mW) and converted back: the mean power is
/// what a receiver actually integrates, whereas a dB-domain mean is dragged down by deep
/// fades (`-50` and `-60` average to about `-52.6`, not `-55`). A network counts only if
/// seen in at least half of the scans (rounded up), so a barely audible AP that appeared
/// once among many scans neither shows up nor inflates its own average; the average uses
/// only the scans that saw it. Other fields come from the strongest sighting.
pub fn average_scans(scans: &[Vec<AccessPoint>]) -> Vec<AccessPoint> {
    let needed = scans.len().div_ceil(2).max(1);
    let mut order: Vec<MacAddress> = Vec::new();
    for ap in scans.iter().flatten() {
        if !order.contains(&ap.bssid) {
            order.push(ap.bssid);
        }
    }
    order
        .into_iter()
        .filter_map(|bssid| {
            let sightings: Vec<&AccessPoint> = scans.iter().filter_map(|s| s.iter().find(|a| a.bssid == bssid)).collect();
            if sightings.len() < needed {
                return None;
            }
            let mean_mw = sightings.iter().map(|a| dbm_to_mw(f64::from(a.rssi_dbm))).sum::<f64>() / sightings.len() as f64;
            let strongest = sightings.iter().max_by_key(|a| a.rssi_dbm)?;
            let mut ap = (*strongest).clone();
            ap.rssi_dbm = mw_to_dbm(mean_mw).round() as i32;
            Some(ap)
        })
        .collect()
}

/// Collects the scans of one multi-scan measurement point.
#[derive(Debug, Clone, PartialEq)]
pub struct Sampler {
    pub x: f64,
    pub y: f64,
    needed: u8,
    scans: Vec<Vec<AccessPoint>>,
}

impl Sampler {
    pub fn new(x: f64, y: f64, needed: u8) -> Self {
        Self { x, y, needed: needed.clamp(1, MAX_SAMPLES_PER_POINT), scans: Vec::new() }
    }

    /// Adds one scan; `true` once enough have been collected.
    pub fn push(&mut self, aps: Vec<AccessPoint>) -> bool {
        self.scans.push(aps);
        self.scans.len() >= usize::from(self.needed)
    }

    /// `(collected, needed)`.
    pub fn progress(&self) -> (usize, usize) {
        (self.scans.len(), usize::from(self.needed))
    }

    pub fn finish(self) -> Vec<AccessPoint> {
        average_scans(&self.scans)
    }
}

#[cfg(test)]
mod tests;
