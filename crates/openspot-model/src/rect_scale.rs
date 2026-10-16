//! Scale calibration from a rectangle of known real-world size.

use crate::Scale;

/// Largest relative disagreement between the horizontal and vertical pixels-per-meter
/// before the user is warned. Hand-clicked corners are good to a few percent; beyond 5 %
/// the plan is probably distorted, the sizes were swapped, or a corner was misplaced.
pub const ASPECT_WARN_FRACTION: f64 = 0.05;

/// Result of fitting a scale to a rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectScale {
    /// Uniform scale: the geometric mean of the horizontal and vertical estimates, so
    /// neither axis is favoured.
    pub scale: Scale,
    /// `|px_per_m_x / px_per_m_y - 1|`: 0 when the clicked rectangle has exactly the
    /// entered aspect ratio.
    pub aspect_mismatch: f64,
}

impl RectScale {
    /// Whether the pixel aspect ratio differs from the real one by more than `limit`
    /// (a fraction, e.g. [`ASPECT_WARN_FRACTION`]).
    pub fn exceeds(&self, limit: f64) -> bool {
        self.aspect_mismatch > limit
    }
}

/// Fits a scale to opposite corners `a` and `b` (map pixels) of a rectangle that is
/// `width_m` wide and `height_m` tall in real life. `None` for zero-area rectangles or
/// non-positive sizes.
pub fn scale_from_rectangle(a: (f64, f64), b: (f64, f64), width_m: f64, height_m: f64) -> Option<RectScale> {
    let (w_px, h_px) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    if !(width_m > 0.0 && height_m > 0.0 && w_px > 0.0 && h_px > 0.0) {
        return None;
    }
    let (sx, sy) = (w_px / width_m, h_px / height_m);
    let scale = Scale::new((sx * sy).sqrt())?;
    Some(RectScale { scale, aspect_mismatch: (sx / sy - 1.0).abs() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consistent_rectangle_gives_exact_scale_and_no_mismatch() {
        let r = scale_from_rectangle((10.0, 10.0), (210.0, 110.0), 10.0, 5.0).unwrap();
        assert!((r.scale.pixels_per_meter() - 20.0).abs() < 1e-9);
        assert!(r.aspect_mismatch < 1e-12);
        assert!(!r.exceeds(ASPECT_WARN_FRACTION));
    }

    #[test]
    fn corner_order_does_not_matter() {
        let a = scale_from_rectangle((210.0, 110.0), (10.0, 10.0), 10.0, 5.0).unwrap();
        let b = scale_from_rectangle((10.0, 10.0), (210.0, 110.0), 10.0, 5.0).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn distorted_rectangle_uses_geometric_mean_and_reports_mismatch() {
        // 200 px / 10 m = 20 px/m horizontally, 125 px / 5 m = 25 px/m vertically.
        let r = scale_from_rectangle((0.0, 0.0), (200.0, 125.0), 10.0, 5.0).unwrap();
        assert!((r.scale.pixels_per_meter() - (20.0f64 * 25.0).sqrt()).abs() < 1e-9);
        assert!((r.aspect_mismatch - 0.2).abs() < 1e-9);
        assert!(r.exceeds(ASPECT_WARN_FRACTION));
    }

    #[test]
    fn mismatch_just_below_and_above_the_limit() {
        // sx = 20, sy = 20 * 1.04 -> 4 % mismatch (|1/1.04 - 1| = 3.8 %), under 5 %.
        let ok = scale_from_rectangle((0.0, 0.0), (200.0, 104.0), 10.0, 5.0).unwrap();
        assert!(!ok.exceeds(ASPECT_WARN_FRACTION));
        let bad = scale_from_rectangle((0.0, 0.0), (200.0, 110.0), 10.0, 5.0).unwrap();
        assert!(bad.exceeds(ASPECT_WARN_FRACTION));
    }

    #[test]
    fn degenerate_input_is_rejected() {
        assert!(scale_from_rectangle((0.0, 0.0), (100.0, 0.0), 10.0, 5.0).is_none());
        assert!(scale_from_rectangle((0.0, 0.0), (0.0, 100.0), 10.0, 5.0).is_none());
        assert!(scale_from_rectangle((0.0, 0.0), (10.0, 10.0), 0.0, 5.0).is_none());
        assert!(scale_from_rectangle((0.0, 0.0), (10.0, 10.0), 5.0, -1.0).is_none());
        assert!(scale_from_rectangle((0.0, 0.0), (10.0, 10.0), f64::NAN, 1.0).is_none());
    }
}
