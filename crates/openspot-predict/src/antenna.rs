//! Transmit antenna gain toward a direction.
//!
//! The parametric directional pattern follows 3GPP TR 36.814 Table A.2.1.1-2 (public):
//! `A_H(phi) = -min(12 (phi/phi_3dB)^2, A_m)`, `A_V(theta) = -min(12 ((theta-tilt)/theta_3dB)^2, A_m)`,
//! `A = -min(-(A_H + A_V), A_m)`; peak gain is added on top. The parabola is a standard
//! fit near the main lobe; it is not a model of any specific product.

use openspot_model::{AntennaPattern, Radio};

/// Direction from the transmitter to a receiver, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Direction {
    /// `atan2(dy, dx)` in plan coordinates.
    pub azimuth_deg: f64,
    /// Angle above the horizontal plane (negative = below the transmitter).
    pub elevation_deg: f64,
}

impl Direction {
    pub fn between(dx: f64, dy: f64, dz: f64) -> Self {
        Self { azimuth_deg: dy.atan2(dx).to_degrees(), elevation_deg: dz.atan2(dx.hypot(dy)).to_degrees() }
    }
}

/// Smallest signed difference `a - b` in degrees, in (-180, 180].
pub fn angle_diff_deg(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

/// 3GPP parabolic attenuation (dB, <= 0) for an off-axis angle.
fn parabolic_db(off_axis_deg: f64, beamwidth_deg: f64, max_attenuation_db: f64) -> f64 {
    -(12.0 * (off_axis_deg / beamwidth_deg).powi(2)).min(max_attenuation_db)
}

/// Gain in dBi of `radio` toward `dir`.
pub fn gain_dbi(radio: &Radio, dir: Direction) -> f64 {
    let peak = radio.antenna_gain_dbi;
    match &radio.pattern {
        AntennaPattern::Omni => peak,
        AntennaPattern::Directional { azimuth_deg, h_beamwidth_deg, v_beamwidth_deg, tilt_deg, front_to_back_db } => {
            let a_h = parabolic_db(angle_diff_deg(dir.azimuth_deg, *azimuth_deg), *h_beamwidth_deg, *front_to_back_db);
            // Downtilt points the boresight below horizontal, i.e. at elevation -tilt.
            let a_v = parabolic_db(dir.elevation_deg + tilt_deg, *v_beamwidth_deg, *front_to_back_db);
            peak - (-(a_h + a_v)).min(*front_to_back_db)
        }
        AntennaPattern::Table { azimuth_deg, tilt_deg, table } => {
            peak + table.azimuth_gain_db(angle_diff_deg(dir.azimuth_deg, *azimuth_deg)) + table.elevation_gain_db(dir.elevation_deg + tilt_deg)
        }
    }
}

#[cfg(test)]
mod tests {
    use openspot_model::{AntennaTable, Band};

    use super::*;

    fn directional(az: f64, tilt: f64) -> Radio {
        let mut r = Radio::new(Band::Ghz5, 36, 20, 20.0);
        r.antenna_gain_dbi = 8.0;
        r.pattern = AntennaPattern::Directional { azimuth_deg: az, h_beamwidth_deg: 60.0, v_beamwidth_deg: 40.0, tilt_deg: tilt, front_to_back_db: 25.0 };
        r
    }

    fn dir(az: f64, el: f64) -> Direction {
        Direction { azimuth_deg: az, elevation_deg: el }
    }

    #[test]
    fn omni_is_constant() {
        let r = Radio::new(Band::Ghz2_4, 1, 20, 20.0);
        for (a, e) in [(0.0, 0.0), (123.0, -80.0), (-170.0, 45.0)] {
            assert_eq!(gain_dbi(&r, dir(a, e)), r.antenna_gain_dbi);
        }
    }

    #[test]
    fn directional_known_angles() {
        let r = directional(90.0, 0.0);
        assert!((gain_dbi(&r, dir(90.0, 0.0)) - 8.0).abs() < 1e-12); // boresight: peak
        assert!((gain_dbi(&r, dir(120.0, 0.0)) - 5.0).abs() < 1e-12); // half the beamwidth: -3 dB
        assert!((gain_dbi(&r, dir(60.0, 0.0)) - 5.0).abs() < 1e-12); // symmetric
        assert!((gain_dbi(&r, dir(150.0, 0.0)) - -4.0).abs() < 1e-12); // one beamwidth: -12 dB
        assert!((gain_dbi(&r, dir(270.0, 0.0)) - -17.0).abs() < 1e-12); // back: capped at -25 dB
        assert!((gain_dbi(&r, dir(90.0, 20.0)) - 5.0).abs() < 1e-12); // v half-beamwidth: -3 dB
        // Combined off-axis in both planes: -3 - 3 = -6 dB.
        assert!((gain_dbi(&r, dir(120.0, 20.0)) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn azimuth_wraps_across_the_seam() {
        let r = directional(175.0, 0.0);
        assert!((gain_dbi(&r, dir(-175.0, 0.0)) - gain_dbi(&r, dir(185.0, 0.0))).abs() < 1e-12);
        assert!((gain_dbi(&r, dir(-175.0, 0.0)) - (8.0 - 12.0 * (10.0f64 / 60.0).powi(2))).abs() < 1e-12);
    }

    #[test]
    fn downtilt_moves_the_vertical_peak_below_horizon() {
        let r = directional(0.0, 10.0);
        assert!((gain_dbi(&r, dir(0.0, -10.0)) - 8.0).abs() < 1e-12);
        assert!(gain_dbi(&r, dir(0.0, -10.0)) > gain_dbi(&r, dir(0.0, 0.0)));
        assert!(gain_dbi(&r, dir(0.0, 0.0)) > gain_dbi(&r, dir(0.0, 10.0)));
    }

    #[test]
    fn table_pattern_adds_both_cuts_to_peak() {
        let mut r = Radio::new(Band::Ghz5, 36, 20, 20.0);
        r.antenna_gain_dbi = 6.0;
        r.pattern = AntennaPattern::Table {
            azimuth_deg: 30.0,
            tilt_deg: 0.0,
            table: AntennaTable {
                name: "t".into(),
                azimuth: vec![(-180.0, -20.0), (0.0, 0.0), (180.0, -20.0)],
                elevation: vec![(-90.0, -18.0), (0.0, 0.0), (90.0, -18.0)],
            },
        };
        assert!((gain_dbi(&r, dir(30.0, 0.0)) - 6.0).abs() < 1e-12);
        // 90 deg off boresight azimuth (-10 dB), 45 deg elevation (-9 dB).
        assert!((gain_dbi(&r, dir(120.0, 45.0)) - (6.0 - 10.0 - 9.0)).abs() < 1e-12);
    }

    #[test]
    fn direction_from_offsets() {
        let d = Direction::between(1.0, 1.0, 2f64.sqrt());
        assert!((d.azimuth_deg - 45.0).abs() < 1e-9 && (d.elevation_deg - 45.0).abs() < 1e-9);
        let below = Direction::between(0.0, 0.0, -3.0);
        assert!((below.elevation_deg - -90.0).abs() < 1e-9);
        assert!((angle_diff_deg(-170.0, 170.0) - 20.0).abs() < 1e-12);
    }
}
