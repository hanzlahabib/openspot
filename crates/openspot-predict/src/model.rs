//! Log-distance path loss with explicit wall and floor terms.
//!
//! `PL(d) = FSPL(d0 = 1 m, f) + 10 n log10(d / d0) + sum(wall losses) + sum(floor losses) + offset`
//!
//! This is the multi-wall form used for indoor planning (ITU-R P.1238 distance power loss
//! coefficient plus a floor-penetration term; COST 231 multi-wall). Here walls are counted
//! explicitly, so `n` is the exponent of the *remaining* distance dependence and is
//! normally lower than the all-in coefficients tabulated in P.1238.

use openspot_model::SPEED_OF_LIGHT;

/// Reference distance of the log-distance term (m). The free-space term is evaluated here.
pub const REFERENCE_DISTANCE_M: f64 = 1.0;

/// Starting exponent before calibration. Between free space (2) and the 2.8-3.3 range
/// P.1238 lists for office/commercial areas, because walls are modelled separately.
/// A prior, not a measurement: calibrate with measured data.
pub const DEFAULT_EXPONENT: f64 = 2.5;

/// Free-space path loss in dB for distance `d_m` at `freq_mhz`: `20 log10(4 pi d f / c)`.
pub fn fspl_db(d_m: f64, freq_mhz: f64) -> f64 {
    20.0 * (4.0 * std::f64::consts::PI * d_m * freq_mhz * 1e6 / SPEED_OF_LIGHT).log10()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropagationModel {
    /// Path-loss exponent `n` of the log-distance term.
    pub exponent: f64,
    /// Extra loss added to every path (dB); the calibration intercept. Negative values
    /// mean the environment is more transparent than the model assumes.
    pub offset_db: f64,
}

impl Default for PropagationModel {
    fn default() -> Self {
        Self { exponent: DEFAULT_EXPONENT, offset_db: 0.0 }
    }
}

impl PropagationModel {
    /// Distance-dependent loss only (no walls/floors), with `d` clamped to the reference
    /// distance so receivers on top of the transmitter see the 1 m loss, never a gain.
    pub fn distance_loss_db(&self, d_m: f64, freq_mhz: f64) -> f64 {
        let d = d_m.max(REFERENCE_DISTANCE_M);
        fspl_db(REFERENCE_DISTANCE_M, freq_mhz) + 10.0 * self.exponent * (d / REFERENCE_DISTANCE_M).log10() + self.offset_db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fspl_at_one_meter_matches_the_analytic_value() {
        // 20 log10(4 pi * 1 m * f / c), computed independently per band center frequency.
        let expected = |f_hz: f64| 20.0 * (4.0 * std::f64::consts::PI * f_hz / 299_792_458.0).log10();
        for (mhz, db) in [(2437.0, 40.2), (5500.0, 47.2), (6500.0, 48.7)] {
            let got = fspl_db(1.0, mhz);
            assert!((got - expected(mhz * 1e6)).abs() < 1e-9);
            assert!((got - db).abs() < 0.06, "{mhz} MHz -> {got}"); // rounded literature check
        }
    }

    #[test]
    fn doubling_distance_adds_ten_n_log_two() {
        for n in [2.0, 2.5, 3.3] {
            let m = PropagationModel { exponent: n, offset_db: 0.0 };
            let d = m.distance_loss_db(8.0, 5180.0) - m.distance_loss_db(4.0, 5180.0);
            assert!((d - 10.0 * n * 2f64.log10()).abs() < 1e-9);
        }
        // Free space (n = 2) adds 6.02 dB per doubling, matching the 20 log10 form of FSPL.
        let free = PropagationModel { exponent: 2.0, offset_db: 0.0 };
        assert!((free.distance_loss_db(20.0, 2437.0) - fspl_db(20.0, 2437.0)).abs() < 1e-9);
    }

    #[test]
    fn near_field_is_clamped_and_offset_is_additive() {
        let m = PropagationModel { exponent: 3.0, offset_db: 4.0 };
        assert_eq!(m.distance_loss_db(0.0, 2437.0), m.distance_loss_db(1.0, 2437.0));
        assert!((m.distance_loss_db(1.0, 2437.0) - (fspl_db(1.0, 2437.0) + 4.0)).abs() < 1e-12);
    }
}
