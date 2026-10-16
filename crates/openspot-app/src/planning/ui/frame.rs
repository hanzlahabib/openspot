//! Plan-meter / screen transform of the planning canvas.

use eframe::egui::Pos2;
use openspot_model::Point;

use crate::planning::PICK_TOLERANCE_PX;
use crate::survey::View;

/// Where the plan sits on screen: the shared pan/zoom [`View`] over map pixels, the
/// canvas origin and the zone's scale (map pixels per meter).
#[derive(Debug, Clone, Copy)]
pub(super) struct Frame {
    pub origin: Pos2,
    pub view: View,
    pub pixels_per_meter: f64,
}

impl Frame {
    pub fn to_screen(self, p: Point) -> Pos2 {
        self.view.to_screen(self.origin, (p.x * self.pixels_per_meter, p.y * self.pixels_per_meter))
    }

    pub fn to_plan(self, screen: Pos2) -> Point {
        let (x, y) = self.view.to_map(self.origin, screen);
        Point::new(x / self.pixels_per_meter, y / self.pixels_per_meter)
    }

    /// Screen length of `meters` at the current zoom.
    pub fn length_px(self, meters: f64) -> f32 {
        (meters * self.pixels_per_meter) as f32 * self.view.zoom
    }

    /// The pick and snap tolerance in meters: a fixed number of screen pixels.
    pub fn tolerance_m(self) -> f64 {
        PICK_TOLERANCE_PX / (f64::from(self.view.zoom) * self.pixels_per_meter)
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Vec2, pos2};

    use super::*;

    fn frame() -> Frame {
        Frame { origin: pos2(100.0, 50.0), view: View { offset: Vec2::new(12.0, -4.0), zoom: 2.0 }, pixels_per_meter: 40.0 }
    }

    #[test]
    fn meters_map_to_screen_through_scale_and_zoom() {
        let f = frame();
        // 1 m = 40 map px = 80 screen px at 2x zoom.
        let (a, b) = (f.to_screen(Point::new(0.0, 0.0)), f.to_screen(Point::new(1.0, 0.0)));
        assert_eq!(a, pos2(112.0, 46.0));
        assert!((b.x - a.x - 80.0).abs() < 1e-3 && (b.y - a.y).abs() < 1e-3);
        assert!((f.length_px(1.0) - 80.0).abs() < 1e-3);
    }

    #[test]
    fn screen_plan_round_trip() {
        let f = frame();
        let p = Point::new(3.25, 7.5);
        let back = f.to_plan(f.to_screen(p));
        assert!((back.x - p.x).abs() < 1e-4 && (back.y - p.y).abs() < 1e-4);
    }

    #[test]
    fn tolerance_is_a_fixed_number_of_screen_pixels() {
        let mut f = frame();
        let tol = f.tolerance_m();
        assert!((f.length_px(tol) - PICK_TOLERANCE_PX as f32).abs() < 1e-3);
        f.view.zoom = 8.0;
        assert!(f.tolerance_m() < tol, "zooming in makes the pick distance smaller in meters");
        assert!((f.length_px(f.tolerance_m()) - PICK_TOLERANCE_PX as f32).abs() < 1e-3);
    }
}
