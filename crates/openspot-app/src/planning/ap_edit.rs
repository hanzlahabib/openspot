//! Pure helpers behind the access-point settings panel: radios, widths and channels,
//! antenna pattern kinds and value ranges.

use openspot_model::{AntennaPattern, Band, DEFAULT_FRONT_TO_BACK_DB, Radio, VirtualAp};

use super::channels;

pub const TX_POWER_RANGE_DBM: std::ops::RangeInclusive<f64> = -10.0..=40.0;
pub const ANTENNA_GAIN_RANGE_DBI: std::ops::RangeInclusive<f64> = -10.0..=30.0;
pub const HEIGHT_RANGE_M: std::ops::RangeInclusive<f64> = 0.1..=50.0;
/// Narrowest beam offered; the engine's parabolic pattern is meaningless below a few degrees.
pub const MIN_BEAMWIDTH_DEG: f64 = 5.0;

/// Transmit power given to a radio added by hand: a typical enterprise setting.
const NEW_RADIO_TX_DBM: f64 = 17.0;
/// Antenna gain given to a radio added by hand: a typical internal omni antenna.
const NEW_RADIO_GAIN_DBI: f64 = 3.0;

/// Which kind of pattern a radio uses, for the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    Omni,
    Directional,
    /// Imported table; cannot be created in the panel, only kept or replaced.
    Table,
}

impl PatternKind {
    pub fn of(p: &AntennaPattern) -> Self {
        match p {
            AntennaPattern::Omni => Self::Omni,
            AntennaPattern::Directional { .. } => Self::Directional,
            AntennaPattern::Table { .. } => Self::Table,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Omni => "Omnidirectional",
            Self::Directional => "Directional",
            Self::Table => "Imported table",
        }
    }
}

/// A directional pattern with typical sector-antenna beamwidths, pointing along +x.
pub fn default_directional() -> AntennaPattern {
    AntennaPattern::Directional { azimuth_deg: 0.0, h_beamwidth_deg: 90.0, v_beamwidth_deg: 60.0, tilt_deg: 0.0, front_to_back_db: DEFAULT_FRONT_TO_BACK_DB }
}

/// Switches the pattern kind, keeping the current pattern when it already is that kind.
pub fn set_pattern_kind(radio: &mut Radio, kind: PatternKind) {
    if PatternKind::of(&radio.pattern) == kind {
        return;
    }
    match kind {
        PatternKind::Omni => radio.pattern = AntennaPattern::Omni,
        PatternKind::Directional => radio.pattern = default_directional(),
        PatternKind::Table => {}
    }
}

/// Wraps an angle in degrees into `(-180, 180]`.
pub fn normalize_azimuth(deg: f64) -> f64 {
    let a = deg.rem_euclid(360.0);
    if a > 180.0 { a - 360.0 } else { a }
}

/// Changes the channel width and moves the channel to the nearest one valid at that width.
/// Returns false (leaving the radio alone) if the band does not support the width.
pub fn set_width(radio: &mut Radio, width_mhz: u16) -> bool {
    match channels::nearest_channel(radio.band, width_mhz, radio.channel) {
        Some(ch) => {
            radio.width_mhz = width_mhz;
            radio.channel = ch;
            true
        }
        None => false,
    }
}

/// Clamps every adjustable value of `radio` into its allowed range, so a typed number or an
/// old project can never hand the engine something it rejects.
pub fn sanitize(radio: &mut Radio) {
    if !channels::is_valid(radio.band, radio.width_mhz, radio.channel) {
        // An unsupported width falls back to 20 MHz; then the channel moves to the nearest valid one.
        if channels::channels(radio.band, radio.width_mhz).is_empty() {
            radio.width_mhz = 20;
        }
        if let Some(ch) = channels::nearest_channel(radio.band, radio.width_mhz, radio.channel) {
            radio.channel = ch;
        }
    }
    let finite = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
    radio.tx_power_dbm = finite(radio.tx_power_dbm, NEW_RADIO_TX_DBM).clamp(*TX_POWER_RANGE_DBM.start(), *TX_POWER_RANGE_DBM.end());
    radio.antenna_gain_dbi = finite(radio.antenna_gain_dbi, NEW_RADIO_GAIN_DBI).clamp(*ANTENNA_GAIN_RANGE_DBI.start(), *ANTENNA_GAIN_RANGE_DBI.end());
    if let AntennaPattern::Directional { azimuth_deg, h_beamwidth_deg, v_beamwidth_deg, tilt_deg, front_to_back_db } = &mut radio.pattern {
        *azimuth_deg = normalize_azimuth(finite(*azimuth_deg, 0.0));
        *h_beamwidth_deg = finite(*h_beamwidth_deg, 90.0).clamp(MIN_BEAMWIDTH_DEG, 360.0);
        *v_beamwidth_deg = finite(*v_beamwidth_deg, 60.0).clamp(MIN_BEAMWIDTH_DEG, 180.0);
        *tilt_deg = finite(*tilt_deg, 0.0).clamp(-90.0, 90.0);
        *front_to_back_db = finite(*front_to_back_db, DEFAULT_FRONT_TO_BACK_DB).clamp(0.0, 60.0);
    }
}

/// Bands the AP has no radio for yet.
pub fn missing_bands(ap: &VirtualAp) -> Vec<Band> {
    Band::ALL.into_iter().filter(|b| !ap.radios.iter().any(|r| r.band == *b)).collect()
}

/// A new radio on the first 20 MHz channel of `band`, omnidirectional.
pub fn new_radio(band: Band) -> Radio {
    let channel = channels::channels(band, 20).first().copied().unwrap_or(1);
    Radio { antenna_gain_dbi: NEW_RADIO_GAIN_DBI, ..Radio::new(band, channel, 20, NEW_RADIO_TX_DBM) }
}

/// One-line description for lists and tooltips, e.g. `5 GHz ch 42 (80 MHz) 20 dBm`.
pub fn radio_summary(r: &Radio) -> String {
    format!("{} ch {} ({} MHz) {:.0} dBm{}", r.band.label(), r.channel, r.width_mhz, r.tx_power_dbm, if r.enabled { "" } else { " off" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::Point;

    #[test]
    fn changing_width_keeps_a_valid_nearby_channel() {
        let mut r = Radio::new(Band::Ghz5, 36, 20, 20.0);
        assert!(set_width(&mut r, 80));
        assert_eq!((r.width_mhz, r.channel), (80, 42));
        assert!(channels::is_valid(r.band, r.width_mhz, r.channel));
        assert!(set_width(&mut r, 20));
        assert_eq!((r.width_mhz, r.channel), (20, 40), "back to the 20 MHz channel nearest the block center (tie to the lower)");
        let mut r24 = Radio::new(Band::Ghz2_4, 6, 20, 17.0);
        assert!(!set_width(&mut r24, 80));
        assert_eq!((r24.width_mhz, r24.channel), (20, 6), "unsupported width changes nothing");
    }

    #[test]
    fn pattern_kinds_switch_and_keep_their_settings() {
        let mut r = Radio::new(Band::Ghz5, 36, 20, 20.0);
        assert_eq!(PatternKind::of(&r.pattern), PatternKind::Omni);
        set_pattern_kind(&mut r, PatternKind::Directional);
        assert!(matches!(r.pattern, AntennaPattern::Directional { h_beamwidth_deg, .. } if h_beamwidth_deg == 90.0));
        if let AntennaPattern::Directional { azimuth_deg, .. } = &mut r.pattern {
            *azimuth_deg = 45.0;
        }
        set_pattern_kind(&mut r, PatternKind::Directional);
        assert!(matches!(r.pattern, AntennaPattern::Directional { azimuth_deg, .. } if azimuth_deg == 45.0), "re-selecting keeps the tuned pattern");
        set_pattern_kind(&mut r, PatternKind::Omni);
        assert_eq!(r.pattern, AntennaPattern::Omni);
        set_pattern_kind(&mut r, PatternKind::Table);
        assert_eq!(r.pattern, AntennaPattern::Omni, "a table cannot be conjured from nothing");
    }

    #[test]
    fn azimuth_wraps_into_a_half_open_turn() {
        for (input, want) in [(0.0, 0.0), (180.0, 180.0), (-180.0, 180.0), (190.0, -170.0), (-190.0, 170.0), (360.0, 0.0), (725.0, 5.0)] {
            assert!((normalize_azimuth(input) - want).abs() < 1e-9, "{input}");
        }
    }

    #[test]
    fn sanitize_clamps_into_ranges_the_engine_accepts() {
        let mut r = Radio::new(Band::Ghz5, 36, 20, 99.0);
        r.antenna_gain_dbi = f64::NAN;
        r.pattern = AntennaPattern::Directional { azimuth_deg: 270.0, h_beamwidth_deg: 0.0, v_beamwidth_deg: 500.0, tilt_deg: 200.0, front_to_back_db: -3.0 };
        sanitize(&mut r);
        assert_eq!((r.width_mhz, r.channel), (20, 36), "a valid channel is untouched");
        assert_eq!(r.tx_power_dbm, 40.0);
        assert_eq!(r.antenna_gain_dbi, 3.0);
        assert_eq!(r.pattern, AntennaPattern::Directional { azimuth_deg: -90.0, h_beamwidth_deg: 5.0, v_beamwidth_deg: 180.0, tilt_deg: 90.0, front_to_back_db: 0.0 });
        // The result is accepted by the engine's validation.
        let ap = VirtualAp::new("a", Point::default(), vec![r]);
        let tx = openspot_predict::Scene::transmitters(&[&openspot_model::PlanningData { access_points: vec![ap], ..Default::default() }]);
        assert!(tx.is_ok());
    }

    #[test]
    fn sanitize_repairs_impossible_channels_and_widths() {
        let mut r = Radio::new(Band::Ghz5, 37, 20, 20.0);
        sanitize(&mut r);
        assert_eq!((r.width_mhz, r.channel), (20, 36), "channel 37 does not exist; 36 is the lower neighbour at a tie");
        let mut wide = Radio::new(Band::Ghz2_4, 6, 160, 17.0);
        sanitize(&mut wide);
        assert_eq!((wide.width_mhz, wide.channel), (20, 6), "2.4 GHz has no 160 MHz channels");
        let mut off = Radio::new(Band::Ghz5, 40, 80, 20.0);
        sanitize(&mut off);
        assert!(channels::is_valid(Band::Ghz5, 80, off.channel));
    }

    #[test]
    fn missing_bands_and_new_radios() {
        let ap = VirtualAp::new("a", Point::default(), vec![Radio::new(Band::Ghz5, 36, 20, 20.0)]);
        assert_eq!(missing_bands(&ap), vec![Band::Ghz2_4, Band::Ghz6]);
        for band in Band::ALL {
            let r = new_radio(band);
            assert!(channels::is_valid(band, r.width_mhz, r.channel) && r.enabled);
        }
        assert_eq!(new_radio(Band::Ghz6).channel, 1);
        assert_eq!(radio_summary(&Radio::new(Band::Ghz5, 42, 80, 20.0)), "5 GHz ch 42 (80 MHz) 20 dBm");
    }
}
