//! Deterministic indoor propagation prediction for OpenSpot planning.
//!
//! See `docs/PLANNING_ENGINE.md` for the model, conventions and API tour.

#![forbid(unsafe_code)]

pub mod antenna;
mod calibrate;
mod error;
pub mod geom;
mod grid;
pub mod placement;
mod reflect;
pub mod model;
mod scene;

pub use calibrate::{Calibration, CalibrationError, MAX_PLAUSIBLE_EXPONENT, MIN_LOG_DISTANCE_SPREAD_DB, MIN_POINTS, Measurement, calibrate};
pub use error::PredictError;
pub use model::{DEFAULT_EXPONENT, PropagationModel, REFERENCE_DISTANCE_M, fspl_db};
pub use grid::{CancelToken, Control, DEFAULT_RX_HEIGHT_M, GridRequest, MAX_CELLS, Mode, PredictionSet, Progress, RadioGrid, rssi_dbm, rssi_grid};
pub use reflect::rssi_dbm_single_bounce;
pub use scene::{PathLoss, Position, Scene, Transmitter};
