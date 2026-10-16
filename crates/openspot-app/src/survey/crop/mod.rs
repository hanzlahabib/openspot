//! Heatmap crop area and gray-out of unmeasured regions (V-15).

mod control;
mod mask;

use openspot_model::CropArea;

pub use mask::{GrayOut, crop_grid, tint_unmeasured};

type Point = (f64, f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CropMode {
    #[default]
    Rect,
    Polygon,
}

/// A crop being drawn: the clicks so far.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CropDraft {
    pub mode: CropMode,
    pub points: Vec<Point>,
}

impl CropDraft {
    pub fn set_mode(&mut self, mode: CropMode) {
        if self.mode != mode {
            self.mode = mode;
            self.points.clear();
        }
    }

    /// Feeds a map click. A rectangle completes on its second click; a polygon only adds a
    /// vertex (finish it with [`Self::finish`]).
    pub fn click(&mut self, p: Point) -> Option<CropArea> {
        match self.mode {
            CropMode::Rect => match self.points.first() {
                Some(&first) => {
                    self.points.clear();
                    Some(CropArea::rect(first, p))
                }
                None => {
                    self.points.push(p);
                    None
                }
            },
            CropMode::Polygon => {
                self.points.push(p);
                None
            }
        }
    }

    /// Closes a polygon; `None` (draft kept) with fewer than three vertices.
    pub fn finish(&mut self) -> Option<CropArea> {
        if self.mode != CropMode::Polygon {
            return None;
        }
        let area = CropArea::polygon(self.points.clone())?;
        self.points.clear();
        Some(area)
    }

    pub fn remove_last(&mut self) -> bool {
        self.points.pop().is_some()
    }

    pub fn reset(&mut self) {
        self.points.clear();
    }
}

#[cfg(test)]
mod tests;
