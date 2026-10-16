//! Scale calibration: click two points, enter the real distance between them.

use openspot_model::{RectScale, Scale, scale_from_rectangle};

type Point = (f64, f64);

/// Meters pre-filled in the dialog; a typical interior door-to-door distance.
const DEFAULT_METERS: f64 = 5.0;
/// Rectangle sizes pre-filled in the dialog; a small room.
const DEFAULT_RECT_METERS: (f64, f64) = (5.0, 3.0);

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Calibration {
    #[default]
    Idle,
    PickFirst,
    PickSecond { a: Point },
    Enter { a: Point, b: Point, meters: f64 },
    /// Rectangle calibration: waiting for the first corner.
    RectFirst,
    RectSecond { a: Point },
    /// Opposite corners chosen; the user enters the real width (along x) and height.
    RectEnter { a: Point, b: Point, width_m: f64, height_m: f64 },
}

impl Calibration {
    pub fn is_active(&self) -> bool {
        *self != Self::Idle
    }

    pub fn start(&mut self) {
        *self = Self::PickFirst;
    }

    pub fn start_rect(&mut self) {
        *self = Self::RectFirst;
    }

    pub fn cancel(&mut self) {
        *self = Self::Idle;
    }

    /// Feeds a map click. Ignored once both points are chosen (the dialog is open).
    pub fn click(&mut self, p: Point) {
        *self = match *self {
            Self::PickFirst => Self::PickSecond { a: p },
            Self::PickSecond { a } => Self::Enter { a, b: p, meters: DEFAULT_METERS },
            Self::RectFirst => Self::RectSecond { a: p },
            Self::RectSecond { a } => {
                Self::RectEnter { a, b: p, width_m: DEFAULT_RECT_METERS.0, height_m: DEFAULT_RECT_METERS.1 }
            }
            other => other,
        };
    }

    /// Completes calibration; `None` (and the state is kept) if the input is unusable.
    pub fn confirm(&mut self) -> Option<Scale> {
        let Self::Enter { a, b, meters } = *self else { return None };
        let scale = Scale::from_reference(a, b, meters)?;
        *self = Self::Idle;
        Some(scale)
    }

    /// Completes rectangle calibration; `None` (and the state is kept) if the input is
    /// unusable. The caller decides what to do with an aspect-ratio mismatch.
    pub fn confirm_rect(&mut self) -> Option<RectScale> {
        let Self::RectEnter { a, b, width_m, height_m } = *self else { return None };
        let fit = scale_from_rectangle(a, b, width_m, height_m)?;
        *self = Self::Idle;
        Some(fit)
    }
}

/// Aspect mismatch of the rectangle currently being entered (for the live warning).
pub(crate) fn rect_mismatch(c: &Calibration) -> Option<f64> {
    let Calibration::RectEnter { a, b, width_m, height_m } = *c else { return None };
    scale_from_rectangle(a, b, width_m, height_m).map(|f| f.aspect_mismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_clicks_then_confirm() {
        let mut c = Calibration::default();
        assert!(!c.is_active());
        c.start();
        c.click((0.0, 0.0));
        assert_eq!(c, Calibration::PickSecond { a: (0.0, 0.0) });
        c.click((100.0, 0.0));
        let Calibration::Enter { meters, .. } = &mut c else { panic!("expected Enter") };
        *meters = 4.0;
        let scale = c.confirm().unwrap();
        assert_eq!(scale.pixels_per_meter(), 25.0);
        assert_eq!(c, Calibration::Idle);
    }

    #[test]
    fn zero_length_or_zero_meters_is_rejected_and_kept() {
        let mut c = Calibration::Enter { a: (1.0, 1.0), b: (1.0, 1.0), meters: 5.0 };
        assert!(c.confirm().is_none());
        assert!(c.is_active());
        c = Calibration::Enter { a: (0.0, 0.0), b: (9.0, 0.0), meters: 0.0 };
        assert!(c.confirm().is_none());
    }

    #[test]
    fn clicks_are_ignored_when_idle_or_in_dialog() {
        let mut c = Calibration::Idle;
        c.click((1.0, 1.0));
        assert_eq!(c, Calibration::Idle);
        let e = Calibration::Enter { a: (0.0, 0.0), b: (1.0, 0.0), meters: 2.0 };
        let mut c2 = e;
        c2.click((5.0, 5.0));
        assert_eq!(c2, e);
    }

    #[test]
    fn rectangle_flow_fits_scale_and_returns_to_idle() {
        let mut c = Calibration::default();
        c.start_rect();
        assert!(c.is_active());
        c.click((0.0, 0.0));
        assert_eq!(c, Calibration::RectSecond { a: (0.0, 0.0) });
        c.click((100.0, 60.0));
        let Calibration::RectEnter { width_m, height_m, .. } = &mut c else { panic!("expected RectEnter") };
        (*width_m, *height_m) = (10.0, 6.0);
        let fit = c.confirm_rect().unwrap();
        assert!((fit.scale.pixels_per_meter() - 10.0).abs() < 1e-9);
        assert_eq!(c, Calibration::Idle);
    }

    #[test]
    fn rectangle_with_zero_area_is_rejected_and_kept() {
        let mut c = Calibration::RectEnter { a: (0.0, 0.0), b: (50.0, 0.0), width_m: 5.0, height_m: 3.0 };
        assert!(c.confirm_rect().is_none());
        assert!(c.is_active());
        // The line flow's confirm must not accept a rectangle state.
        assert!(c.confirm().is_none());
    }

    #[test]
    fn cancel_resets() {
        let mut c = Calibration::PickFirst;
        c.cancel();
        assert_eq!(c, Calibration::Idle);
    }
}
