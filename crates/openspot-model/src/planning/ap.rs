//! Virtual access points and their antennas.

use serde::{Deserialize, Serialize};

use super::material::Band;
use super::structure::Point;
use crate::Uuid;

/// Typical front-to-back ratio; also the 3GPP TR 36.814 sector-antenna maximum attenuation.
pub const DEFAULT_FRONT_TO_BACK_DB: f64 = 25.0;

/// Error from [`AntennaTable::validate`].
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AntennaError {
    #[error("antenna file is not valid JSON: {0}")]
    Parse(String),
    #[error("antenna cut '{0}' needs at least two points")]
    TooFewPoints(&'static str),
    #[error("antenna cut '{0}' has a non-finite value or angles that are not strictly increasing")]
    BadCut(&'static str),
    #[error("antenna azimuth cut must lie within one turn (-180..=360)")]
    AzimuthRange,
    #[error("antenna elevation cut must lie within -90..=90")]
    ElevationRange,
}

/// Gain tables relative to the peak gain, in dB (<= 0 normally; 0 at boresight).
///
/// JSON format (angles in degrees, gains in dB relative to peak):
/// `{"name":"..","azimuth":[[-180,-25],[0,0],[180,-25]],"elevation":[[-90,-20],[0,0],[90,-20]]}`.
/// Azimuth is measured from boresight, positive in the direction of increasing atan2(dy, dx) in plan coordinates, and wraps; elevation
/// is measured from the boresight plane, positive up. Gains are linearly interpolated in dB.
/// The 3-D gain is approximated as the sum of the two cuts (separable pattern).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AntennaTable {
    pub name: String,
    pub azimuth: Vec<(f64, f64)>,
    pub elevation: Vec<(f64, f64)>,
}

impl AntennaTable {
    pub fn from_json(json: &str) -> Result<Self, AntennaError> {
        let table: Self = serde_json::from_str(json).map_err(|e| AntennaError::Parse(e.to_string()))?;
        table.validate()?;
        Ok(table)
    }

    pub fn validate(&self) -> Result<(), AntennaError> {
        for (cut, name) in [(&self.azimuth, "azimuth"), (&self.elevation, "elevation")] {
            if cut.len() < 2 {
                return Err(AntennaError::TooFewPoints(name));
            }
            let finite = cut.iter().all(|(a, g)| a.is_finite() && g.is_finite());
            if !finite || cut.windows(2).any(|w| w[1].0 <= w[0].0) {
                return Err(AntennaError::BadCut(name));
            }
        }
        if self.azimuth.iter().any(|&(a, _)| !(-180.0..=360.0).contains(&a)) || self.azimuth_span() > 360.0 {
            return Err(AntennaError::AzimuthRange);
        }
        if self.elevation.iter().any(|&(a, _)| !(-90.0..=90.0).contains(&a)) {
            return Err(AntennaError::ElevationRange);
        }
        Ok(())
    }

    fn azimuth_span(&self) -> f64 {
        match (self.azimuth.first(), self.azimuth.last()) {
            (Some(f), Some(l)) => l.0 - f.0,
            _ => 0.0,
        }
    }

    /// Relative azimuth gain (dB) at `angle_deg` from boresight; periodic.
    pub fn azimuth_gain_db(&self, angle_deg: f64) -> f64 {
        let first = self.azimuth[0].0;
        let a = first + (angle_deg - first).rem_euclid(360.0);
        let last = *self.azimuth.last().unwrap_or(&self.azimuth[0]);
        if a <= last.0 {
            return interpolate(&self.azimuth, a);
        }
        // Between the last sample and the first one a turn later.
        let (f, l) = (self.azimuth[0], last);
        let span = f.0 + 360.0 - l.0;
        l.1 + (f.1 - l.1) * (a - l.0) / span
    }

    /// Relative elevation gain (dB); clamped to the end samples outside the table.
    pub fn elevation_gain_db(&self, angle_deg: f64) -> f64 {
        interpolate(&self.elevation, angle_deg)
    }
}

/// Piecewise-linear interpolation over strictly increasing x; clamps outside.
fn interpolate(points: &[(f64, f64)], x: f64) -> f64 {
    let (first, last) = (points[0], points[points.len() - 1]);
    if x <= first.0 {
        return first.1;
    }
    if x >= last.0 {
        return last.1;
    }
    let i = points.partition_point(|p| p.0 <= x);
    let (a, b) = (points[i - 1], points[i]);
    a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AntennaPattern {
    /// Same gain in every direction (an idealisation: real "omni" antennas null overhead).
    Omni,
    /// Parametric directional pattern, 3GPP TR 36.814 A.2.1.1 style (parabolic in dB).
    Directional {
        /// Boresight bearing in degrees, as atan2(dy, dx) in plan coordinates (clockwise on screen, since y points down).
        azimuth_deg: f64,
        /// Horizontal half-power (3 dB) beamwidth.
        h_beamwidth_deg: f64,
        /// Vertical half-power beamwidth.
        v_beamwidth_deg: f64,
        /// Downtilt of boresight below horizontal (positive = pointing down).
        tilt_deg: f64,
        /// Maximum attenuation relative to peak (back lobe level).
        front_to_back_db: f64,
    },
    /// Measured/imported pattern.
    Table { azimuth_deg: f64, tilt_deg: f64, table: AntennaTable },
}

/// One radio (one band) of a virtual AP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Radio {
    pub band: Band,
    /// Center channel number (IEEE numbering for the band).
    pub channel: u16,
    pub width_mhz: u16,
    pub tx_power_dbm: f64,
    pub antenna_gain_dbi: f64,
    pub pattern: AntennaPattern,
    pub enabled: bool,
}

impl Radio {
    pub fn new(band: Band, channel: u16, width_mhz: u16, tx_power_dbm: f64) -> Self {
        Self { band, channel, width_mhz, tx_power_dbm, antenna_gain_dbi: 3.0, pattern: AntennaPattern::Omni, enabled: true }
    }

    pub fn center_frequency_mhz(&self) -> f64 {
        self.band.channel_center_mhz(self.channel)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VirtualAp {
    pub id: Uuid,
    pub name: String,
    pub position: Point,
    /// Mounting height above the level's floor (m).
    pub height_m: f64,
    pub radios: Vec<Radio>,
}

/// Ceiling-mount height, only a default.
pub const DEFAULT_AP_HEIGHT_M: f64 = 2.5;

impl VirtualAp {
    pub fn new(name: impl Into<String>, position: Point, radios: Vec<Radio>) -> Self {
        Self { id: Uuid::new_v4(), name: name.into(), position, height_m: DEFAULT_AP_HEIGHT_M, radios }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> AntennaTable {
        AntennaTable::from_json(
            r#"{"name":"t","azimuth":[[-180,-20],[-90,-10],[0,0],[90,-10],[180,-20]],"elevation":[[-90,-30],[0,0],[90,-30]]}"#,
        )
        .unwrap()
    }

    #[test]
    fn table_interpolates_in_db_and_wraps() {
        let t = table();
        assert_eq!(t.azimuth_gain_db(0.0), 0.0);
        assert!((t.azimuth_gain_db(45.0) - -5.0).abs() < 1e-12);
        assert!((t.azimuth_gain_db(-45.0) - -5.0).abs() < 1e-12);
        assert!((t.azimuth_gain_db(360.0)).abs() < 1e-12);
        assert!((t.azimuth_gain_db(270.0) - -10.0).abs() < 1e-12); // = -90
        assert!((t.azimuth_gain_db(180.0) - -20.0).abs() < 1e-12);
        assert!((t.elevation_gain_db(45.0) - -15.0).abs() < 1e-12);
        assert!((t.elevation_gain_db(-200.0) - -30.0).abs() < 1e-12); // clamped
    }

    #[test]
    fn table_wraps_across_a_partial_range() {
        // 0..270 only: the gap 270..360 is interpolated back to the first sample.
        let t = AntennaTable {
            name: "p".into(),
            azimuth: vec![(0.0, 0.0), (270.0, -9.0)],
            elevation: vec![(-90.0, 0.0), (90.0, 0.0)],
        };
        t.validate().unwrap();
        assert!((t.azimuth_gain_db(315.0) - -4.5).abs() < 1e-12);
    }

    #[test]
    fn malformed_tables_are_rejected() {
        assert!(matches!(AntennaTable::from_json("nope"), Err(AntennaError::Parse(_))));
        let bad = |az: &str, el: &str| AntennaTable::from_json(&format!(r#"{{"name":"x","azimuth":{az},"elevation":{el}}}"#));
        let ok_el = "[[-90,0],[90,0]]";
        assert_eq!(bad("[[0,0]]", ok_el), Err(AntennaError::TooFewPoints("azimuth")));
        assert_eq!(bad("[[10,0],[10,-1]]", ok_el), Err(AntennaError::BadCut("azimuth")));
        assert_eq!(bad("[[0,0],[400,-1]]", ok_el), Err(AntennaError::AzimuthRange));
        assert_eq!(bad("[[0,0],[10,-1]]", "[[-100,0],[0,0]]"), Err(AntennaError::ElevationRange));
        assert_eq!(bad("[[0,0],[10,-1]]", "[[0,0]]"), Err(AntennaError::TooFewPoints("elevation")));
    }
}
