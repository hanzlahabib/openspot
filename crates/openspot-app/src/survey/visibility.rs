//! What the canvas draws (V-22): AP marker size, sample indicator detail.

use openspot_model::ScanPoint;

/// Size of AP markers. "None" is the existing AP-positions checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApMarkerSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl ApMarkerSize {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }

    /// Half-diagonal of the diamond in screen px. Medium is the long-standing size; the
    /// others are about 0.65x and 1.45x so the steps are clearly visible.
    pub fn radius_px(self) -> f32 {
        match self {
            Self::Small => 6.0,
            Self::Medium => 9.0,
            Self::Large => 13.0,
        }
    }
}

/// How much each measurement point shows. "None" is the existing Points checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleDetail {
    /// A colored dot.
    #[default]
    Simple,
    /// The dot plus the strongest signal measured there.
    Detailed,
}

impl SampleDetail {
    pub const ALL: [Self; 2] = [Self::Simple, Self::Detailed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Simple => "Simple",
            Self::Detailed => "Detailed (with signal)",
        }
    }
}

/// Caption of a point in detailed mode: its strongest RSSI, or a dash if nothing was heard.
pub fn sample_caption(point: &ScanPoint) -> String {
    point.access_points.iter().map(|a| a.rssi_dbm).max().map_or_else(|| "–".to_owned(), |v| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_wlan::{AccessPoint, MacAddress};

    fn ap(last: u8, rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
    }

    #[test]
    fn marker_sizes_grow_and_medium_is_the_default() {
        let r: Vec<f32> = ApMarkerSize::ALL.iter().map(|s| s.radius_px()).collect();
        assert!(r.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(ApMarkerSize::default().radius_px(), 9.0);
    }

    #[test]
    fn caption_is_the_strongest_reading() {
        let p = ScanPoint::new(0.0, 0.0, 0, vec![ap(1, -70), ap(2, -48), ap(3, -90)]);
        assert_eq!(sample_caption(&p), "-48");
        assert_eq!(sample_caption(&ScanPoint::new(0.0, 0.0, 0, vec![])), "–");
    }
}
