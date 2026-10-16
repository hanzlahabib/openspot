//! Pan/zoom transform between map-image pixels and screen points.

use eframe::egui::{Pos2, Vec2};
#[cfg(test)]
use eframe::egui::pos2;

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 20.0;
/// Margin kept around the map when fitting it to the canvas.
const FIT_MARGIN: f32 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Screen offset of the map's top-left corner from the canvas origin.
    pub offset: Vec2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self { offset: Vec2::ZERO, zoom: 1.0 }
    }
}

impl View {
    /// Largest zoom that shows the whole map, centered in `canvas`.
    pub fn fit(map: Vec2, canvas: Vec2) -> Self {
        if map.x <= 0.0 || map.y <= 0.0 || canvas.x <= 0.0 || canvas.y <= 0.0 {
            return Self::default();
        }
        let zoom = ((canvas.x / map.x).min(canvas.y / map.y) * FIT_MARGIN).clamp(MIN_ZOOM, MAX_ZOOM);
        Self { offset: (canvas - map * zoom) / 2.0, zoom }
    }

    pub fn to_screen(self, origin: Pos2, map: (f64, f64)) -> Pos2 {
        origin + self.offset + Vec2::new(map.0 as f32, map.1 as f32) * self.zoom
    }

    pub fn to_map(self, origin: Pos2, screen: Pos2) -> (f64, f64) {
        let v = (screen - origin - self.offset) / self.zoom;
        (f64::from(v.x), f64::from(v.y))
    }

    /// Zooms by `factor` keeping the map point under `cursor` fixed.
    pub fn zoom_at(&mut self, origin: Pos2, cursor: Pos2, factor: f32) {
        let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let rel = cursor - origin;
        self.offset = rel - (rel - self.offset) / self.zoom * new_zoom;
        self.zoom = new_zoom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(x: f32, y: f32) -> Pos2 {
        pos2(x, y)
    }

    #[test]
    fn fit_centers_and_scales() {
        let v = View::fit(Vec2::new(100.0, 50.0), Vec2::new(400.0, 400.0));
        assert!((v.zoom - 4.0 * FIT_MARGIN).abs() < 1e-5);
        let o = origin(10.0, 10.0);
        let center = v.to_screen(o, (50.0, 25.0));
        assert!((center.x - 210.0).abs() < 1e-3 && (center.y - 210.0).abs() < 1e-3);
    }

    #[test]
    fn screen_map_round_trip() {
        let v = View { offset: Vec2::new(13.0, -7.0), zoom: 2.5 };
        let o = origin(100.0, 50.0);
        let s = v.to_screen(o, (40.0, 30.0));
        let (x, y) = v.to_map(o, s);
        assert!((x - 40.0).abs() < 1e-4 && (y - 30.0).abs() < 1e-4);
    }

    #[test]
    fn zoom_keeps_cursor_point_fixed() {
        let mut v = View { offset: Vec2::new(20.0, 30.0), zoom: 1.0 };
        let o = origin(0.0, 0.0);
        let cursor = pos2(200.0, 150.0);
        let before = v.to_map(o, cursor);
        v.zoom_at(o, cursor, 1.7);
        let after = v.to_map(o, cursor);
        assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut v = View::default();
        v.zoom_at(origin(0.0, 0.0), pos2(0.0, 0.0), 1e9);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_at(origin(0.0, 0.0), pos2(0.0, 0.0), 1e-9);
        assert_eq!(v.zoom, MIN_ZOOM);
    }

    #[test]
    fn degenerate_fit_is_default() {
        assert_eq!(View::fit(Vec2::ZERO, Vec2::new(10.0, 10.0)), View::default());
    }
}
