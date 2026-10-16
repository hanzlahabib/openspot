//! Building geometry of one planned level: walls, openings, cutouts, slab.
//! All coordinates are meters in the zone's plan frame (x right, y down, like the map).

use serde::{Deserialize, Serialize};

use crate::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallKind {
    Wall,
    Door,
    Window,
    /// Shelf or rack.
    Shelf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    Door,
    Window,
}

/// A door or window set into a wall, spanning a fraction `[start, end]` of its length
/// (0 = wall start, 1 = wall end). A signal crossing the wall inside the span loses the
/// opening's material instead of the wall's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub id: Uuid,
    pub kind: OpeningKind,
    pub start: f64,
    pub end: f64,
    pub material: String,
}

impl Opening {
    /// Whether the wall position `u` (0..1) lies within the opening.
    pub fn contains(&self, u: f64) -> bool {
        u >= self.start && u <= self.end
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wall {
    pub id: Uuid,
    pub a: Point,
    pub b: Point,
    /// Physical thickness, for drawing and for materials defined per thickness.
    pub thickness_m: f64,
    pub kind: WallKind,
    /// Material id: the zone's custom materials first, then the preset catalog.
    pub material: String,
    pub openings: Vec<Opening>,
}

impl Wall {
    pub fn new(a: Point, b: Point, kind: WallKind, material: impl Into<String>) -> Self {
        Self { id: Uuid::new_v4(), a, b, thickness_m: 0.1, kind, material: material.into(), openings: Vec::new() }
    }

    pub fn length_m(&self) -> f64 {
        (self.b.x - self.a.x).hypot(self.b.y - self.a.y)
    }

    /// Material crossed at wall position `u`: an opening's if one covers it.
    pub fn material_at(&self, u: f64) -> &str {
        self.openings.iter().find(|o| o.contains(u)).map_or(self.material.as_str(), |o| o.material.as_str())
    }
}

/// An opening through the floor slab (stairwell, atrium). The polygon is in the
/// coordinates of the level that owns the slab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cutout {
    pub id: Uuid,
    pub polygon: Vec<Point>,
}

/// The floor slab ("overlap") under a level, separating it from the level below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slab {
    pub thickness_m: f64,
    pub material: String,
}

impl Default for Slab {
    fn default() -> Self {
        Self { thickness_m: 0.2, material: "concrete-slab".into() }
    }
}

/// Vertical placement of a level in its building and its alignment to the others.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Level {
    /// Height of this level's floor above the building reference (m).
    pub elevation_m: f64,
    /// Floor-to-floor height (m).
    pub floor_height_m: f64,
    /// Added to this level's plan coordinates to align it with the others.
    pub offset: Point,
    pub slab: Slab,
}

/// A typical storey; only a starting value.
// One shared default for survey buildings and planning levels.
use crate::structure::DEFAULT_FLOOR_HEIGHT_M;

impl Default for Level {
    fn default() -> Self {
        Self { elevation_m: 0.0, floor_height_m: DEFAULT_FLOOR_HEIGHT_M, offset: Point::default(), slab: Slab::default() }
    }
}
