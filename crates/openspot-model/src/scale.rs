/// Unit used to enter and show real-world distances. Storage is always meters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LengthUnit {
    #[default]
    Meters,
    Centimeters,
    Feet,
    Inches,
}

impl LengthUnit {
    pub const ALL: [Self; 4] = [Self::Meters, Self::Centimeters, Self::Feet, Self::Inches];

    pub fn label(self) -> &'static str {
        match self {
            Self::Meters => "m",
            Self::Centimeters => "cm",
            Self::Feet => "ft",
            Self::Inches => "in",
        }
    }

    /// Exact by definition (international foot and inch).
    pub fn meters_per_unit(self) -> f64 {
        match self {
            Self::Meters => 1.0,
            Self::Centimeters => 0.01,
            Self::Feet => 0.3048,
            Self::Inches => 0.0254,
        }
    }

    pub fn to_meters(self, value: f64) -> f64 {
        value * self.meters_per_unit()
    }

    pub fn from_meters(self, meters: f64) -> f64 {
        meters / self.meters_per_unit()
    }
}

/// Pixels-per-meter of a floor plan, set by calibrating against a known distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pixels_per_meter: f64,
}

impl Scale {
    /// `None` unless the value is finite and positive.
    pub fn new(pixels_per_meter: f64) -> Option<Self> {
        (pixels_per_meter.is_finite() && pixels_per_meter > 0.0).then_some(Self { pixels_per_meter })
    }

    /// Calibrates from two map points a known real-world distance apart.
    pub fn from_reference(a: (f64, f64), b: (f64, f64), meters: f64) -> Option<Self> {
        let px = (a.0 - b.0).hypot(a.1 - b.1);
        if meters <= 0.0 || px <= 0.0 {
            return None;
        }
        Self::new(px / meters)
    }

    pub fn pixels_per_meter(self) -> f64 {
        self.pixels_per_meter
    }

    pub fn to_meters(self, pixels: f64) -> f64 {
        pixels / self.pixels_per_meter
    }

    pub fn to_pixels(self, meters: f64) -> f64 {
        meters * self.pixels_per_meter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversions_round_trip_and_match_definitions() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
        assert!(close(LengthUnit::Feet.to_meters(10.0), 3.048));
        assert!(close(LengthUnit::Inches.to_meters(12.0), LengthUnit::Feet.to_meters(1.0)));
        assert!(close(LengthUnit::Centimeters.to_meters(250.0), 2.5));
        for u in LengthUnit::ALL {
            let back = u.from_meters(u.to_meters(7.25));
            assert!((back - 7.25).abs() < 1e-12, "{u:?}");
        }
        assert_eq!(LengthUnit::ALL.map(LengthUnit::label), ["m", "cm", "ft", "in"]);
    }

    #[test]
    fn calibration_from_reference() {
        let s = Scale::from_reference((0.0, 0.0), (300.0, 400.0), 10.0).unwrap();
        assert_eq!(s.pixels_per_meter(), 50.0);
        assert_eq!(s.to_meters(125.0), 2.5);
        assert_eq!(s.to_pixels(2.0), 100.0);
    }

    #[test]
    fn rejects_degenerate_input() {
        assert!(Scale::from_reference((1.0, 1.0), (1.0, 1.0), 5.0).is_none());
        assert!(Scale::from_reference((0.0, 0.0), (5.0, 0.0), 0.0).is_none());
        assert!(Scale::new(f64::NAN).is_none());
        assert!(Scale::new(-1.0).is_none());
    }
}
