//! Turns scattered survey samples into a raster heatmap.
//!
//! Inside the convex hull of the samples values are linearly interpolated over a
//! Delaunay triangulation (exact at sample points, no overshoot). Beyond the hull, up
//! to a configurable radius, inverse-distance weighting extends the map so the edges of
//! a walked path still show coverage.

#![forbid(unsafe_code)]

mod color;
mod contour;
mod grid;
mod interference;
mod job;
mod locate;
mod metric;
mod nearest_index;
mod per_ap;
mod raster;
mod speed;
mod tx_rate;

pub use contour::{Isoline, Polyline, evenly_spaced_levels, isolines};
pub use color::{Gradient, GradientPreset, Palette};
pub use grid::{Grid, GridSpec, Sample, Smoothing, interpolate, interpolate_with, nearest, nearest_with, smooth, smooth_with};
pub use job::{CancelToken, Cancelled, Control, Job, JobPoll, Progress};
pub use locate::{ApEstimate, estimate_ap_position};
pub use interference::{MAX_SIR_DB, NO_SERVICE_SIR_DB, sir_db};
pub use metric::{FLOOR_DBM, Metric, samples};
pub use per_ap::per_ap_grids;
pub use raster::{category_shares, colorize, colorize_ranged, colorize_with, effective_gradient, metric_key, rasterize, rasterize_samples};
pub use speed::{SpeedKind, speed_samples};
pub use tx_rate::{TX_RATE_UNIT, tx_rate_samples};
