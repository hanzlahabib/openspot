//! Drawing tools for plans built inside the app (V-07): vector shapes that are rendered
//! to the zone's map image, so the rest of the app only ever sees an ordinary plan.

mod control;
mod hit;
mod raster;
mod session;
mod text;

use thiserror::Error;

pub use hit::shape_bounds;
pub use raster::render_png;
pub use session::{DrawSession, DrawTool};

/// Largest drawn plan edge in pixels; keeps the render and PNG within a few hundred MB
/// of memory even for a mistyped size.
pub const MAX_DRAWING_SIDE_PX: u32 = 8192;
/// Name given to the map image generated from a drawing.
pub const DRAWN_PLAN_NAME: &str = "drawn plan";

#[derive(Debug, Error)]
pub enum DrawError {
    #[error("the drawing size must be between 1 and {MAX_DRAWING_SIDE_PX} pixels")]
    BadSize,
    #[error("the bundled font could not be loaded")]
    Font,
    #[error("could not encode the plan image: {0}")]
    Encode(#[from] image::ImageError),
}

#[cfg(test)]
mod tests;
