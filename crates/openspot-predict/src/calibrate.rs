//! Fit the model to real measurements (P-07).
//!
//! With known wall and floor losses removed, the model is linear in its two free
//! parameters: `Ptx + G - RSSI - FSPL(1 m) - walls - floors = n * 10 log10(d) + offset`.
//! Ordinary least squares on that line gives the path-loss exponent `n` and the offset.

use thiserror::Error;

use openspot_model::Band;

use crate::error::PredictError;
use crate::grid::tx_gain_dbi;
use crate::model::{PropagationModel, REFERENCE_DISTANCE_M, fspl_db};
use crate::scene::{Position, Scene, Transmitter};

/// Fewest points for a fit with a residual: two parameters, so three points leave one
/// degree of freedom. Fewer would "fit" perfectly whatever the data.
pub const MIN_POINTS: usize = 3;

/// The measured distances must span at least this many dB in `10 log10(d)` (a factor of
/// two in distance). Over a narrower range the slope is dominated by measurement noise and
/// is not separable from the offset.
pub const MIN_LOG_DISTANCE_SPREAD_DB: f64 = 3.0;

/// Fits outside `(0, MAX_PLAUSIBLE_EXPONENT]` are rejected rather than applied. Loss that
/// does not grow with distance is non-physical for these models, and even heavily
/// obstructed indoor links stay well below 8, so such a fit signals wrong inputs
/// (swapped AP, wrong units) rather than an unusual building.
pub const MAX_PLAUSIBLE_EXPONENT: f64 = 8.0;

/// One real reading: what `transmitters[transmitter]` measured at `position`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    pub transmitter: usize,
    pub position: Position,
    pub rssi_dbm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    pub exponent: f64,
    pub offset_db: f64,
    /// Root-mean-square of measured minus fitted, in dB.
    pub rmse_db: f64,
    pub points: usize,
    /// Per-measurement residual (measured RSSI minus fitted RSSI), input order.
    pub residuals_db: Vec<f64>,
}

impl Calibration {
    /// The fitted parameters as a propagation model.
    pub fn model(&self) -> PropagationModel {
        PropagationModel { exponent: self.exponent, offset_db: self.offset_db }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum CalibrationError {
    #[error("calibration needs at least {MIN_POINTS} measurements, got {0}")]
    TooFewPoints(usize),
    #[error("measurement {0} is not finite or refers to an unknown transmitter")]
    InvalidMeasurement(usize),
    #[error("measurements are all at about the same distance from their transmitters; spread them over at least a 2:1 range of distances")]
    DegenerateGeometry,
    #[error("fitted path-loss exponent {0:.2} is outside the plausible range; check the measurements and transmitter settings")]
    ImplausibleExponent(f64),
    #[error(transparent)]
    Predict(#[from] PredictError),
}

/// Least-squares calibration of the path-loss exponent and offset.
pub fn calibrate(scene: &Scene, transmitters: &[Transmitter], measurements: &[Measurement]) -> Result<Calibration, CalibrationError> {
    if measurements.len() < MIN_POINTS {
        return Err(CalibrationError::TooFewPoints(measurements.len()));
    }
    // Only the wall/floor/distance terms are used, which do not depend on the model parameters.
    let probe = PropagationModel::default();
    let mut xs = Vec::with_capacity(measurements.len());
    let mut ys = Vec::with_capacity(measurements.len());
    for (i, m) in measurements.iter().enumerate() {
        let tx = transmitters.get(m.transmitter).filter(|_| m.rssi_dbm.is_finite()).ok_or(CalibrationError::InvalidMeasurement(i))?;
        let freq = tx.center_frequency_mhz();
        let band: Band = tx.radio.band;
        let loss = scene.path_loss(&probe, tx.position, m.position, freq, band)?;
        let gain = tx_gain_dbi(scene, tx, m.position)?;
        let known = fspl_db(REFERENCE_DISTANCE_M, freq) + loss.wall_db + loss.floor_db;
        xs.push(10.0 * (loss.distance_m.max(REFERENCE_DISTANCE_M) / REFERENCE_DISTANCE_M).log10());
        ys.push(tx.radio.tx_power_dbm + gain - m.rssi_dbm - known);
    }

    let n = xs.len() as f64;
    let (mean_x, mean_y) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    if hi - lo < MIN_LOG_DISTANCE_SPREAD_DB {
        return Err(CalibrationError::DegenerateGeometry);
    }
    let sxx: f64 = xs.iter().map(|x| (x - mean_x).powi(2)).sum();
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mean_x) * (y - mean_y)).sum();
    let exponent = sxy / sxx;
    if !(exponent > 0.0 && exponent <= MAX_PLAUSIBLE_EXPONENT) {
        return Err(CalibrationError::ImplausibleExponent(exponent));
    }
    let offset_db = mean_y - exponent * mean_x;
    // Loss residual (fitted loss - measured loss) equals measured - fitted RSSI.
    let residuals_db: Vec<f64> = xs.iter().zip(&ys).map(|(x, y)| (exponent * x + offset_db) - y).collect();
    let rmse_db = (residuals_db.iter().map(|r| r * r).sum::<f64>() / n).sqrt();
    Ok(Calibration { exponent, offset_db, rmse_db, points: measurements.len(), residuals_db })
}

#[cfg(test)]
mod tests;
