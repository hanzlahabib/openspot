//! Generic access-point models and the user's own saved models.
//!
//! Provenance policy (also in `docs/PLANNING_UI.md`): OpenSpot ships **no vendor or product
//! catalog**. Datasheet figures cannot be sourced and kept accurate here, and a wrong
//! antenna or power figure silently produces a wrong coverage map. The built-in models
//! below are round-number *typical classes* of access point chosen by this project, not
//! the specification of any product. Users who know their hardware save their own models.

use openspot_model::{AntennaPattern, Band, DEFAULT_AP_HEIGHT_M, DEFAULT_FRONT_TO_BACK_DB, PlanningData, Point, Radio, VirtualAp};
use serde::{Deserialize, Serialize};

/// Longest model name accepted; keeps list entries and the prefs file tidy.
pub const MAX_NAME_LEN: usize = 60;

/// A reusable AP configuration: radios and mounting height, without identity or position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApModel {
    pub name: String,
    pub height_m: f64,
    pub radios: Vec<Radio>,
}

impl ApModel {
    /// A new AP (fresh id) at `position` using this model's settings.
    pub fn instantiate(&self, name: impl Into<String>, position: Point) -> VirtualAp {
        let mut ap = VirtualAp::new(name, position, self.radios.clone());
        ap.height_m = self.height_m;
        ap
    }

    /// The reusable settings of an existing AP.
    pub fn from_ap(name: impl Into<String>, ap: &VirtualAp) -> Self {
        Self { name: name.into(), height_m: ap.height_m, radios: ap.radios.clone() }
    }
}

fn radio(band: Band, channel: u16, width_mhz: u16, tx_power_dbm: f64, antenna_gain_dbi: f64) -> Radio {
    Radio { antenna_gain_dbi, ..Radio::new(band, channel, width_mhz, tx_power_dbm) }
}

fn sector(mut r: Radio, h_beamwidth_deg: f64, v_beamwidth_deg: f64) -> Radio {
    r.pattern = AntennaPattern::Directional { azimuth_deg: 0.0, h_beamwidth_deg, v_beamwidth_deg, tilt_deg: 0.0, front_to_back_db: DEFAULT_FRONT_TO_BACK_DB };
    r
}

/// The built-in generic models. Names say "Generic" on purpose.
pub fn builtin_models() -> Vec<ApModel> {
    vec![
        ApModel {
            name: "Generic Wi-Fi 6 indoor AP".into(),
            height_m: DEFAULT_AP_HEIGHT_M,
            radios: vec![radio(Band::Ghz2_4, 1, 20, 17.0, 3.0), radio(Band::Ghz5, 42, 80, 20.0, 4.0)],
        },
        ApModel {
            name: "Generic Wi-Fi 6E indoor AP".into(),
            height_m: DEFAULT_AP_HEIGHT_M,
            radios: vec![radio(Band::Ghz2_4, 1, 20, 17.0, 3.0), radio(Band::Ghz5, 42, 80, 20.0, 4.0), radio(Band::Ghz6, 7, 80, 20.0, 4.0)],
        },
        ApModel {
            name: "Generic wall-mount directional AP".into(),
            height_m: 2.2,
            radios: vec![radio(Band::Ghz2_4, 1, 20, 15.0, 6.0), sector(radio(Band::Ghz5, 42, 80, 18.0, 8.0), 90.0, 60.0)],
        },
        ApModel {
            name: "Generic outdoor directional".into(),
            height_m: 5.0,
            radios: vec![sector(radio(Band::Ghz5, 149, 20, 23.0, 13.0), 60.0, 30.0)],
        },
    ]
}

/// Builtin plus user models, for pickers. Builtins come first and cannot be replaced.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelLibrary {
    builtin: Vec<ApModel>,
    pub user: Vec<ApModel>,
}

/// Why a model could not be saved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("give the model a name")]
    EmptyName,
    #[error("'{0}' is a built-in model name; choose another")]
    BuiltinName(String),
}

impl ModelLibrary {
    pub fn new(user: Vec<ApModel>) -> Self {
        Self { builtin: builtin_models(), user }
    }

    pub fn builtin(&self) -> &[ApModel] {
        &self.builtin
    }

    pub fn all(&self) -> impl Iterator<Item = &ApModel> {
        self.builtin.iter().chain(&self.user)
    }

    /// Saves `model` under its name, replacing a user model of the same name.
    pub fn save_user(&mut self, mut model: ApModel) -> Result<(), ModelError> {
        let name: String = model.name.trim().chars().take(MAX_NAME_LEN).collect();
        if name.is_empty() {
            return Err(ModelError::EmptyName);
        }
        if self.builtin.iter().any(|m| m.name.eq_ignore_ascii_case(&name)) {
            return Err(ModelError::BuiltinName(name));
        }
        model.name = name;
        match self.user.iter_mut().find(|m| m.name.eq_ignore_ascii_case(&model.name)) {
            Some(slot) => *slot = model,
            None => self.user.push(model),
        }
        Ok(())
    }

    pub fn delete_user(&mut self, name: &str) -> bool {
        let before = self.user.len();
        self.user.retain(|m| m.name != name);
        self.user.len() != before
    }
}

/// "AP 1", "AP 2", ...: the lowest number not used by an AP of this level yet.
pub fn next_ap_name(data: &PlanningData) -> String {
    let taken = |n: usize| data.access_points.iter().any(|ap| ap.name == format!("AP {n}"));
    format!("AP {}", (1..).find(|&n| !taken(n)).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::channels;

    #[test]
    fn builtin_models_are_generic_valid_and_sane() {
        let models = builtin_models();
        assert!(models.len() >= 3);
        for m in &models {
            assert!(m.name.starts_with("Generic"), "{}: no product names", m.name);
            assert!(m.height_m > 0.0 && !m.radios.is_empty());
            for r in &m.radios {
                assert!(channels::is_valid(r.band, r.width_mhz, r.channel), "{}: {:?} ch {} @ {}", m.name, r.band, r.channel, r.width_mhz);
                assert!(r.enabled && (0.0..=36.0).contains(&r.tx_power_dbm) && (0.0..=20.0).contains(&r.antenna_gain_dbi));
            }
        }
        let names: std::collections::BTreeSet<_> = models.iter().map(|m| &m.name).collect();
        assert_eq!(names.len(), models.len(), "unique names");
        assert!(models.iter().any(|m| m.radios.iter().any(|r| matches!(r.pattern, AntennaPattern::Directional { .. }))));
    }

    #[test]
    fn instantiating_gives_fresh_independent_aps() {
        let m = &builtin_models()[0];
        let (a, b) = (m.instantiate("A", Point::new(1.0, 2.0)), m.instantiate("B", Point::new(3.0, 4.0)));
        assert_ne!(a.id, b.id);
        assert_eq!((a.position, a.height_m, &a.radios), (Point::new(1.0, 2.0), m.height_m, &m.radios));
        let back = ApModel::from_ap("copy", &a);
        assert_eq!((back.radios, back.height_m), (m.radios.clone(), m.height_m));
    }

    #[test]
    fn user_models_save_replace_and_delete_but_never_shadow_builtins() {
        let mut lib = ModelLibrary::new(vec![]);
        let mut mine = builtin_models()[0].clone();
        mine.name = "  My lab AP  ".into();
        lib.save_user(mine.clone()).unwrap();
        assert_eq!(lib.user[0].name, "My lab AP");
        mine.name = "my LAB ap".into();
        mine.height_m = 3.3;
        lib.save_user(mine).unwrap();
        assert_eq!((lib.user.len(), lib.user[0].height_m), (1, 3.3), "same name (any case) replaces");
        assert_eq!(lib.user[0].name, "my LAB ap", "the replacement carries the new spelling");
        let mut clash = builtin_models()[1].clone();
        clash.name = "generic WI-FI 6E indoor ap".into();
        assert!(matches!(lib.save_user(clash), Err(ModelError::BuiltinName(_))));
        let mut empty = builtin_models()[0].clone();
        empty.name = "   ".into();
        assert_eq!(lib.save_user(empty), Err(ModelError::EmptyName));
        assert!(lib.delete_user("my LAB ap") && !lib.delete_user("my LAB ap"));
        assert_eq!(lib.all().count(), lib.builtin().len());
    }

    #[test]
    fn ap_names_fill_the_lowest_free_number() {
        let mut data = PlanningData::default();
        assert_eq!(next_ap_name(&data), "AP 1");
        for n in [1, 2, 4] {
            data.access_points.push(VirtualAp::new(format!("AP {n}"), Point::default(), vec![]));
        }
        assert_eq!(next_ap_name(&data), "AP 3");
    }

    #[test]
    fn models_survive_a_json_round_trip() {
        let models = builtin_models();
        let json = serde_json::to_string(&models).unwrap();
        assert_eq!(serde_json::from_str::<Vec<ApModel>>(&json).unwrap(), models);
    }
}
