//! RSSI grids per radio, and combinations of them (strongest AP, AP count, SNR, SIR).
//!
//! Grids use `openspot_heatmap::Grid` (row-major, `cell_px` in map pixels) so the existing
//! colorizer renders predictions. Prediction coordinates are meters; `pixels_per_meter`
//! converts the cell size to pixels. Cell `(col, row)` is sampled at its center.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use openspot_heatmap::{FLOOR_DBM, Grid, MAX_SIR_DB};
use openspot_model::{Band, Uuid};

use crate::antenna::{Direction, gain_dbi};
use crate::error::PredictError;
use crate::model::PropagationModel;
use crate::reflect::rssi_dbm_single_bounce;
use crate::scene::{Position, Scene, Transmitter};

/// Upper bound on cells in one grid. 4 M cells is a 2 km x 2 km plan at 1 m cells or
/// a 200 m x 200 m plan at 10 cm; beyond it a request is almost certainly a unit mistake
/// and would exhaust memory (4 bytes x 4 M x radios).
pub const MAX_CELLS: usize = 4_000_000;

/// Typical handheld/laptop height used when the caller has no better figure.
pub const DEFAULT_RX_HEIGHT_M: f64 = 1.0;

/// Cooperative cancellation shared between the caller and the workers.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Rows finished out of the total. Under the `parallel` feature callbacks may arrive from
/// worker threads in any order, but `done` values are unique and reach `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

/// Optional cancellation token and progress callback for long computations.
#[derive(Default)]
pub struct Control<'a> {
    pub cancel: Option<&'a CancelToken>,
    pub progress: Option<&'a (dyn Fn(Progress) + Sync)>,
}

impl std::fmt::Debug for Control<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control").field("cancel", &self.cancel).field("progress", &self.progress.is_some()).finish()
    }
}

/// The area and resolution to predict, on one level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridRequest {
    pub level: usize,
    pub width_m: f64,
    pub height_m: f64,
    pub cell_m: f64,
    /// Receiver height above the level's floor.
    pub rx_height_m: f64,
    /// Map scale; sets `Grid::cell_px`.
    pub pixels_per_meter: f64,
    pub mode: Mode,
}

/// Which propagation physics a grid uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Direct path through walls and floors (the calibrated multi-wall model).
    #[default]
    Direct,
    /// Direct path plus single-bounce wall reflections (image method, power-summed).
    /// Slower (roughly walls x more work) and not covered by calibration.
    SingleBounce,
}

impl GridRequest {
    pub fn dimensions(&self) -> Result<(usize, usize), PredictError> {
        let ok = [self.width_m, self.height_m, self.cell_m, self.pixels_per_meter].iter().all(|v| v.is_finite() && *v > 0.0);
        if !ok || !self.rx_height_m.is_finite() {
            return Err(PredictError::InvalidInput("grid size, cell size and scale must be positive".into()));
        }
        let cols = (self.width_m / self.cell_m).ceil() as usize;
        let rows = (self.height_m / self.cell_m).ceil() as usize;
        match cols.checked_mul(rows) {
            Some(n) if n <= MAX_CELLS => Ok((cols.max(1), rows.max(1))),
            _ => Err(PredictError::InvalidInput(format!("grid of {cols} x {rows} cells exceeds the {MAX_CELLS} cell limit"))),
        }
    }

    fn cell_center(&self, col: usize, row: usize) -> Position {
        Position {
            level: self.level,
            x: (col as f64 + 0.5) * self.cell_m,
            y: (row as f64 + 0.5) * self.cell_m,
            height_m: self.rx_height_m,
        }
    }
}

/// Received power in dBm at `rx` from `tx`: `Ptx + G(az, el) - PL`.
pub fn rssi_dbm(scene: &Scene, model: &PropagationModel, tx: &Transmitter, rx: Position) -> Result<f64, PredictError> {
    let loss = scene.path_loss(model, tx.position, rx, tx.center_frequency_mhz(), tx.radio.band)?;
    Ok(tx.radio.tx_power_dbm + tx_gain_dbi(scene, tx, rx)? - loss.total_db())
}

/// Transmit antenna gain of `tx` toward `rx`.
pub(crate) fn tx_gain_dbi(scene: &Scene, tx: &Transmitter, rx: Position) -> Result<f64, PredictError> {
    let (dx, dy, dz) = offsets(scene, tx.position, rx)?;
    Ok(gain_dbi(&tx.radio, Direction::between(dx, dy, dz)))
}

/// World-frame offset from `a` to `b` (levels may be shifted by their alignment offsets).
fn offsets(scene: &Scene, a: Position, b: Position) -> Result<(f64, f64, f64), PredictError> {
    let level = |p: Position| scene.levels.get(p.level).ok_or_else(|| PredictError::InvalidInput(format!("no level {}", p.level)));
    let (la, lb) = (level(a)?, level(b)?);
    Ok((
        (b.x + lb.offset.x) - (a.x + la.offset.x),
        (b.y + lb.offset.y) - (a.y + la.offset.y),
        (lb.elevation_m + b.height_m) - (la.elevation_m + a.height_m),
    ))
}

fn cell_rssi(scene: &Scene, model: &PropagationModel, tx: &Transmitter, req: &GridRequest, col: usize, row: usize) -> Result<f64, PredictError> {
    let rx = req.cell_center(col, row);
    match req.mode {
        Mode::Direct => rssi_dbm(scene, model, tx, rx),
        Mode::SingleBounce => rssi_dbm_single_bounce(scene, model, tx, rx),
    }
}

/// RSSI (dBm) of one transmitter over the whole grid; every cell has a value.
pub fn rssi_grid(scene: &Scene, model: &PropagationModel, tx: &Transmitter, req: &GridRequest, control: &Control<'_>) -> Result<Grid, PredictError> {
    let (cols, rows) = req.dimensions()?;
    if req.level >= scene.level_count() {
        return Err(PredictError::InvalidInput(format!("no level {}", req.level)));
    }
    let done = AtomicUsize::new(0);
    let row_values = |row: usize| -> Result<Vec<Option<f32>>, PredictError> {
        if control.cancel.is_some_and(CancelToken::is_cancelled) {
            return Err(PredictError::Cancelled);
        }
        let values = (0..cols)
            .map(|col| cell_rssi(scene, model, tx, req, col, row).map(|v| Some(v as f32)))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(report) = control.progress {
            report(Progress { done: done.fetch_add(1, Ordering::Relaxed) + 1, total: rows });
        }
        Ok(values)
    };
    #[cfg(feature = "parallel")]
    let per_row: Result<Vec<_>, _> = {
        use rayon::prelude::*;
        (0..rows).into_par_iter().map(row_values).collect()
    };
    #[cfg(not(feature = "parallel"))]
    let per_row: Result<Vec<_>, _> = (0..rows).map(row_values).collect();
    let values = per_row?.into_iter().flatten().collect();
    Ok(Grid { cols, rows, cell_px: req.cell_m * req.pixels_per_meter, values })
}

/// The grid of one radio plus what identifies it.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioGrid {
    pub ap_id: Uuid,
    pub radio_index: usize,
    pub band: Band,
    pub center_mhz: f64,
    pub width_mhz: f64,
    pub grid: Grid,
}

/// Predictions for every radio over the same area; the input to all combined views.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictionSet {
    pub radios: Vec<RadioGrid>,
    pub cols: usize,
    pub rows: usize,
    pub cell_px: f64,
}

impl PredictionSet {
    /// Predicts all `transmitters`, one grid each, sharing one cancel token and progress
    /// reporter (progress restarts for each radio's grid).
    pub fn compute(scene: &Scene, model: &PropagationModel, transmitters: &[Transmitter], req: &GridRequest, control: &Control<'_>) -> Result<Self, PredictError> {
        let (cols, rows) = req.dimensions()?;
        let radios = transmitters
            .iter()
            .map(|tx| {
                Ok(RadioGrid {
                    ap_id: tx.ap_id,
                    radio_index: tx.radio_index,
                    band: tx.radio.band,
                    center_mhz: tx.center_frequency_mhz(),
                    width_mhz: f64::from(tx.radio.width_mhz),
                    grid: rssi_grid(scene, model, tx, req, control)?,
                })
            })
            .collect::<Result<Vec<_>, PredictError>>()?;
        Ok(Self { radios, cols, rows, cell_px: req.cell_m * req.pixels_per_meter })
    }

    fn in_band(&self, band: Option<Band>) -> impl Iterator<Item = &RadioGrid> {
        self.radios.iter().filter(move |r| band.is_none_or(|b| r.band == b))
    }

    fn blank(&self) -> Grid {
        Grid { cols: self.cols, rows: self.rows, cell_px: self.cell_px, values: vec![None; self.cols * self.rows] }
    }

    fn value(r: &RadioGrid, i: usize) -> f64 {
        r.grid.values[i].map_or(FLOOR_DBM, f64::from)
    }

    /// Strongest signal at each cell among radios of `band` (all bands if `None`).
    /// Cells stay empty when there is no such radio.
    pub fn strongest(&self, band: Option<Band>) -> Grid {
        let mut out = self.blank();
        for (i, cell) in out.values.iter_mut().enumerate() {
            *cell = self.in_band(band).map(|r| Self::value(r, i)).reduce(f64::max).map(|v| v as f32);
        }
        out
    }

    /// Number of radios of `band` heard at or above `threshold_dbm` at each cell.
    pub fn ap_count(&self, threshold_dbm: f64, band: Option<Band>) -> Grid {
        let mut out = self.blank();
        for (i, cell) in out.values.iter_mut().enumerate() {
            *cell = Some(self.in_band(band).filter(|r| Self::value(r, i) >= threshold_dbm).count() as f32);
        }
        out
    }

    /// Strongest signal minus the noise floor.
    pub fn snr(&self, noise_dbm: f64, band: Option<Band>) -> Grid {
        let mut out = self.strongest(band);
        for v in out.values.iter_mut().flatten() {
            *v -= noise_dbm as f32;
        }
        out
    }

    /// Co-channel signal-to-interference ratio (dB), same model as the survey SIR heatmap
    /// (`openspot_heatmap::sir_db`): the serving radio is the strongest one heard; every
    /// other radio in the same band whose spectrum overlaps adds interference, weighted by
    /// the fraction of the serving bandwidth it covers; powers add in mW; capped at
    /// `MAX_SIR_DB`. Radios below `heard_dbm` are ignored; where none is heard the cell is empty.
    pub fn sir(&self, heard_dbm: f64, band: Option<Band>) -> Grid {
        let mut out = self.blank();
        let radios: Vec<&RadioGrid> = self.in_band(band).collect();
        for (i, cell) in out.values.iter_mut().enumerate() {
            let heard: Vec<(&RadioGrid, f64)> = radios.iter().map(|r| (*r, Self::value(r, i))).filter(|(_, v)| *v >= heard_dbm).collect();
            // First maximum in radio order, so ties resolve deterministically.
            let Some(&(serving, s_dbm)) = heard.iter().reduce(|best, cur| if cur.1 > best.1 { cur } else { best }) else { continue };
            let interference_mw: f64 = heard
                .iter()
                .filter(|(r, _)| !std::ptr::eq(*r, serving))
                .map(|(r, v)| 10f64.powf(v / 10.0) * (overlap_mhz(serving, r) / serving.width_mhz).min(1.0))
                .sum();
            let sir = if interference_mw <= 0.0 { MAX_SIR_DB } else { (s_dbm - 10.0 * interference_mw.log10()).min(MAX_SIR_DB) };
            *cell = Some(sir as f32);
        }
        out
    }

    /// Radio (index into `radios`) serving each cell, as a category grid.
    pub fn serving_radio(&self, band: Option<Band>) -> Grid {
        let mut out = self.blank();
        let radios: Vec<(usize, &RadioGrid)> = self.radios.iter().enumerate().filter(|(_, r)| band.is_none_or(|b| r.band == b)).collect();
        for (i, cell) in out.values.iter_mut().enumerate() {
            *cell = radios
                .iter()
                .map(|(idx, r)| (*idx, Self::value(r, i)))
                .reduce(|best, cur| if cur.1 > best.1 { cur } else { best })
                .map(|(idx, _)| idx as f32);
        }
        out
    }
}

/// MHz of spectrum shared by two radios; zero across bands.
fn overlap_mhz(a: &RadioGrid, b: &RadioGrid) -> f64 {
    if a.band != b.band {
        return 0.0;
    }
    let lo = (a.center_mhz - a.width_mhz / 2.0).max(b.center_mhz - b.width_mhz / 2.0);
    let hi = (a.center_mhz + a.width_mhz / 2.0).min(b.center_mhz + b.width_mhz / 2.0);
    (hi - lo).max(0.0)
}

#[cfg(test)]
mod tests;
