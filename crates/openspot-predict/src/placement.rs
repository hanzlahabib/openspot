//! Greedy AP placement suggestion (P-09).
//!
//! Classic greedy maximum-coverage: repeatedly pick the candidate position whose radio
//! lifts the most not-yet-covered cells to at least the threshold, until `count` APs are
//! placed or nothing improves. Greedy selection of a monotone submodular coverage
//! objective is within `1 - 1/e` of the best possible choice of the same number of
//! candidates (Nemhauser, Wolsey & Fisher 1978). It places APs for *coverage only*;
//! it does not assign channels or consider co-channel interference.

use openspot_model::{Point, Radio, Uuid};

use crate::error::PredictError;
use crate::grid::{Control, GridRequest, rssi_grid};
use crate::model::PropagationModel;
use crate::scene::{Position, Scene, Transmitter};

/// Most candidates considered in one run; each costs one full grid prediction.
pub const MAX_CANDIDATES: usize = 2_000;

#[derive(Debug, Clone, Copy)]
pub struct PlacementRequest<'a> {
    /// Candidate positions in the plan coordinates of `grid.level`.
    pub candidates: &'a [Point],
    /// Radio of the AP to place (band, channel, power, antenna).
    pub template: &'a Radio,
    /// Mounting height of the new APs above the floor.
    pub height_m: f64,
    /// Number of APs to add at most.
    pub count: usize,
    /// A cell is covered when some radio of the template's band reaches this level.
    pub threshold_dbm: f64,
    pub grid: GridRequest,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Suggestion {
    pub position: Point,
    /// Cells this AP newly covers.
    pub newly_covered_cells: usize,
    /// Share of all cells covered after adding this AP and the ones before it.
    pub covered_fraction: f64,
}

/// Regular candidate lattice over `width_m x height_m`, `spacing_m` apart, keeping
/// `margin_m` clear of the edges. Row-major order, so ties resolve reproducibly.
pub fn candidate_lattice(width_m: f64, height_m: f64, spacing_m: f64, margin_m: f64) -> Vec<Point> {
    if !(spacing_m > 0.0 && spacing_m.is_finite()) {
        return Vec::new();
    }
    let steps = |extent: f64| ((extent - 2.0 * margin_m) / spacing_m).floor().max(-1.0) as i64 + 1;
    let (nx, ny) = (steps(width_m), steps(height_m));
    (0..ny.max(0)).flat_map(|j| (0..nx.max(0)).map(move |i| (i, j))).map(|(i, j)| Point::new(margin_m + i as f64 * spacing_m, margin_m + j as f64 * spacing_m)).collect()
}

fn coverage(scene: &Scene, model: &PropagationModel, tx: &Transmitter, req: &GridRequest, threshold_dbm: f64, control: &Control<'_>) -> Result<Vec<bool>, PredictError> {
    let grid = rssi_grid(scene, model, tx, req, control)?;
    Ok(grid.values.iter().map(|v| v.is_some_and(|v| f64::from(v) >= threshold_dbm)).collect())
}

/// Suggests up to `req.count` positions for new APs, given the `existing` transmitters.
pub fn suggest_placements(scene: &Scene, model: &PropagationModel, existing: &[Transmitter], req: &PlacementRequest<'_>, control: &Control<'_>) -> Result<Vec<Suggestion>, PredictError> {
    if req.candidates.len() > MAX_CANDIDATES {
        return Err(PredictError::InvalidInput(format!("{} candidates exceed the limit of {MAX_CANDIDATES}", req.candidates.len())));
    }
    let (cols, rows) = req.grid.dimensions()?;
    let total = cols * rows;

    let mut covered = vec![false; total];
    for tx in existing.iter().filter(|t| t.radio.band == req.template.band) {
        for (c, hit) in covered.iter_mut().zip(coverage(scene, model, tx, &req.grid, req.threshold_dbm, control)?) {
            *c |= hit;
        }
    }
    let masks = req
        .candidates
        .iter()
        .map(|p| {
            let tx = Transmitter {
                ap_id: Uuid::nil(),
                radio_index: 0,
                radio: req.template.clone(),
                position: Position { level: req.grid.level, x: p.x, y: p.y, height_m: req.height_m },
            };
            coverage(scene, model, &tx, &req.grid, req.threshold_dbm, control)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut chosen = vec![false; masks.len()];
    let mut out = Vec::new();
    for _ in 0..req.count {
        let gain = |m: &Vec<bool>| m.iter().zip(&covered).filter(|(h, c)| **h && !**c).count();
        // First best in candidate order, so ties are deterministic.
        let best = masks
            .iter()
            .enumerate()
            .filter(|(i, _)| !chosen[*i])
            .map(|(i, m)| (i, gain(m)))
            .reduce(|best, cur| if cur.1 > best.1 { cur } else { best });
        let Some((i, newly)) = best.filter(|(_, g)| *g > 0) else { break };
        chosen[i] = true;
        for (c, hit) in covered.iter_mut().zip(&masks[i]) {
            *c |= hit;
        }
        let covered_cells = covered.iter().filter(|c| **c).count();
        out.push(Suggestion { position: req.candidates[i], newly_covered_cells: newly, covered_fraction: covered_cells as f64 / total as f64 });
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
