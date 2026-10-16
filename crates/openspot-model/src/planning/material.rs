//! Radio bands, building materials and the versioned preset catalog.
//!
//! A material's `absorption_db` is the loss of *one crossing* of the object, per band.
//! Presets are derived only from public data (ITU-R P.2040); see `data/materials.json`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Speed of light in vacuum, m/s (exact by SI definition).
pub const SPEED_OF_LIGHT: f64 = 299_792_458.0;

/// ITU-R P.2040: imaginary part of relative permittivity is `17.98 * sigma / f_GHz`
/// (`1 / (2 pi eps0)` with f in GHz, rounded as in the Recommendation).
const P2040_LOSS_FACTOR: f64 = 17.98;

/// Wi-Fi band. The 6 GHz band is the 5.925-7.125 GHz U-NII-5..8 range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Band {
    Ghz2_4,
    Ghz5,
    Ghz6,
}

impl Band {
    pub const ALL: [Self; 3] = [Self::Ghz2_4, Self::Ghz5, Self::Ghz6];

    /// Frequency used to evaluate material properties for the band: roughly the band
    /// center of the channels in common use (2.4 GHz ch. 6, 5 GHz mid-band, 6 GHz low-mid).
    pub fn reference_ghz(self) -> f64 {
        match self {
            Self::Ghz2_4 => 2.45,
            Self::Ghz5 => 5.5,
            Self::Ghz6 => 6.5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ghz2_4 => "2.4 GHz",
            Self::Ghz5 => "5 GHz",
            Self::Ghz6 => "6 GHz",
        }
    }

    /// Center frequency in MHz of a channel *number* in this band (IEEE 802.11-2020
    /// 17.3.8.4.2 / 19.3.15 / 802.11ax Annex E): 2.4 GHz `2407 + 5n` (channel 14 is 2484),
    /// 5 GHz `5000 + 5n`, 6 GHz `5950 + 5n`.
    pub fn channel_center_mhz(self, channel: u16) -> f64 {
        let n = f64::from(channel);
        match self {
            Self::Ghz2_4 if channel == 14 => 2484.0,
            Self::Ghz2_4 => 2407.0 + 5.0 * n,
            Self::Ghz5 => 5000.0 + 5.0 * n,
            Self::Ghz6 => 5950.0 + 5.0 * n,
        }
    }
}

/// One value for each band.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PerBand<T = f64> {
    pub ghz2_4: T,
    pub ghz5: T,
    pub ghz6: T,
}

impl<T: Copy> PerBand<T> {
    pub fn splat(v: T) -> Self {
        Self { ghz2_4: v, ghz5: v, ghz6: v }
    }

    pub fn get(&self, band: Band) -> T {
        match band {
            Band::Ghz2_4 => self.ghz2_4,
            Band::Ghz5 => self.ghz5,
            Band::Ghz6 => self.ghz6,
        }
    }
}

/// What a material is used for; the planner offers presets by category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialCategory {
    Wall,
    Door,
    Window,
    Shelf,
    /// Floor slab between levels ("overlap").
    Slab,
}

/// ITU-R P.2040 Table 3 coefficients: relative permittivity `a * f^b`, conductivity
/// `c * f^d` (S/m), with `f` in GHz.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct P2040Params {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// Stable id referenced by walls; catalog ids are lowercase kebab-case.
    pub id: String,
    pub name: String,
    pub category: MaterialCategory,
    /// Thickness the loss values refer to, when derived from a slab model.
    pub reference_thickness_m: Option<f64>,
    /// dB lost per crossing.
    pub absorption_db: PerBand,
    /// Reflection coefficient magnitude, 0 (none) to 1 (perfect mirror).
    pub reflection: PerBand,
    /// Free-text provenance ("custom", or the published source).
    pub source: String,
    pub p2040: Option<P2040Params>,
}

impl Material {
    /// A user-defined material with the same loss and reflection in every band.
    pub fn custom(id: impl Into<String>, name: impl Into<String>, category: MaterialCategory, absorption_db: f64, reflection: f64) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            category,
            reference_thickness_m: None,
            absorption_db: PerBand::splat(absorption_db),
            reflection: PerBand::splat(reflection),
            source: "custom".into(),
            p2040: None,
        }
    }

    /// Derives per-band loss and reflection of a slab of `thickness_m` from ITU-R P.2040
    /// permittivity coefficients.
    pub fn from_p2040(id: impl Into<String>, name: impl Into<String>, category: MaterialCategory, params: P2040Params, thickness_m: f64) -> Self {
        let per_band = |f: fn((f64, f64)) -> f64| {
            let v = |band: Band| f(slab_transmission_reflection(params, band.reference_ghz(), thickness_m));
            PerBand { ghz2_4: v(Band::Ghz2_4), ghz5: v(Band::Ghz5), ghz6: v(Band::Ghz6) }
        };
        Self {
            id: id.into(),
            name: name.into(),
            category,
            reference_thickness_m: Some(thickness_m),
            absorption_db: per_band(|(loss, _)| loss),
            reflection: per_band(|(_, refl)| refl),
            source: "derived from ITU-R P.2040 coefficients".into(),
            p2040: Some(params),
        }
    }

    /// Loss/reflection values must be finite and non-negative, reflection at most 1.
    pub fn is_valid(&self) -> bool {
        Band::ALL.iter().all(|&b| {
            let (a, r) = (self.absorption_db.get(b), self.reflection.get(b));
            a.is_finite() && a >= 0.0 && r.is_finite() && (0.0..=1.0).contains(&r)
        })
    }
}

#[derive(Clone, Copy)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn mul(self, o: Self) -> Self {
        Self { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
    }

    fn sub(self, o: Self) -> Self {
        Self { re: self.re - o.re, im: self.im - o.im }
    }

    fn div(self, o: Self) -> Self {
        let d = o.re * o.re + o.im * o.im;
        Self { re: (self.re * o.re + self.im * o.im) / d, im: (self.im * o.re - self.re * o.im) / d }
    }

    fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// Principal square root.
    fn sqrt(self) -> Self {
        let m = self.abs();
        let re = ((m + self.re) / 2.0).sqrt();
        let im = ((m - self.re) / 2.0).sqrt().copysign(self.im);
        Self { re, im }
    }

    /// `e^(self)`.
    fn exp(self) -> Self {
        let m = self.re.exp();
        Self { re: m * self.im.cos(), im: m * self.im.sin() }
    }
}

/// Transmission loss (dB, >= 0) and reflection magnitude of one homogeneous lossy slab at
/// normal incidence in free space, including multiple internal reflections:
/// `n = sqrt(eps' - j 17.98 sigma / f)`, `G = (1 - n)/(1 + n)`, `E = exp(-2 j k0 n d)`,
/// `T = (1 - G^2) exp(-j k0 n d) / (1 - G^2 E)`, `R = G (1 - E) / (1 - G^2 E)`.
/// (Standard slab result, e.g. Balanis, *Advanced Engineering Electromagnetics* 5.4.)
/// Oblique incidence and frequency-dependent thickness effects are not modelled.
pub fn slab_transmission_reflection(params: P2040Params, freq_ghz: f64, thickness_m: f64) -> (f64, f64) {
    let eps_re = params.a * freq_ghz.powf(params.b);
    let sigma = params.c * freq_ghz.powf(params.d);
    let eps = Complex { re: eps_re, im: -P2040_LOSS_FACTOR * sigma / freq_ghz };
    let n = eps.sqrt();
    let one = Complex { re: 1.0, im: 0.0 };
    let g = one.sub(n).div(Complex { re: 1.0 + n.re, im: n.im });
    let g2 = g.mul(g);
    let k0 = 2.0 * std::f64::consts::PI * freq_ghz * 1e9 / SPEED_OF_LIGHT;
    // exp(-j k0 n d) with n = n' - j n'' (n.im = -n'') decays as exp(-k0 n'' d).
    let phase = Complex { re: k0 * thickness_m * n.im, im: -k0 * thickness_m * n.re };
    let e = phase.mul(Complex { re: 2.0, im: 0.0 }).exp();
    let denom = one.sub(g2.mul(e));
    let t = one.sub(g2).mul(phase.exp()).div(denom);
    let r = g.mul(one.sub(e)).div(denom);
    (-20.0 * t.abs().log10(), r.abs())
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("material catalog is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("material catalog has a duplicate id: {0}")]
    DuplicateId(String),
    #[error("material {0} has a negative, non-finite or out-of-range value")]
    InvalidMaterial(String),
}

/// A versioned set of preset materials. Bump `version` whenever values change so the UI
/// can tell users that presets were updated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialCatalog {
    pub version: u32,
    pub provenance: String,
    pub materials: Vec<Material>,
}

const BUILTIN_JSON: &str = include_str!("../../data/materials.json");

impl MaterialCatalog {
    /// The catalog shipped with the application.
    pub fn builtin() -> Result<Self, CatalogError> {
        Self::from_json(BUILTIN_JSON)
    }

    pub fn from_json(json: &str) -> Result<Self, CatalogError> {
        let catalog: Self = serde_json::from_str(json)?;
        let mut seen = BTreeMap::new();
        for m in &catalog.materials {
            if !m.is_valid() {
                return Err(CatalogError::InvalidMaterial(m.id.clone()));
            }
            if seen.insert(m.id.as_str(), ()).is_some() {
                return Err(CatalogError::DuplicateId(m.id.clone()));
            }
        }
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.id == id)
    }

    pub fn by_category(&self, category: MaterialCategory) -> impl Iterator<Item = &Material> {
        self.materials.iter().filter(move |m| m.category == category)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_parses_and_is_internally_consistent() {
        let c = MaterialCatalog::builtin().unwrap();
        assert_eq!(c.version, 1);
        assert!(c.materials.len() >= 8);
        for m in &c.materials {
            let p = m.p2040.expect("preset has coefficients");
            let t = m.reference_thickness_m.expect("preset has a thickness");
            for band in Band::ALL {
                let (loss, refl) = slab_transmission_reflection(p, band.reference_ghz(), t);
                assert!((loss - m.absorption_db.get(band)).abs() < 0.06, "{} {band:?} loss {loss}", m.id);
                assert!((refl - m.reflection.get(band)).abs() < 0.006, "{} {band:?} refl {refl}", m.id);
            }
        }
    }

    #[test]
    fn zero_thickness_slab_is_transparent_and_loss_grows_with_thickness() {
        let p = MaterialCatalog::builtin().unwrap().get("concrete-wall").unwrap().p2040.unwrap();
        let (loss0, refl0) = slab_transmission_reflection(p, 5.5, 0.0);
        assert!(loss0.abs() < 1e-9 && refl0.abs() < 1e-9);
        let l1 = slab_transmission_reflection(p, 5.5, 0.10).0;
        let l2 = slab_transmission_reflection(p, 5.5, 0.20).0;
        assert!(l2 > l1 && l1 > 0.0);
    }

    #[test]
    fn lossless_dielectric_half_wave_slab_is_transparent() {
        // eps'=4, sigma=0: n=2; a slab of half a guided wavelength (d = lambda/(2n)) has no reflection.
        let p = P2040Params { a: 4.0, b: 0.0, c: 0.0, d: 0.0 };
        let f = 5.0;
        let lambda = SPEED_OF_LIGHT / (f * 1e9);
        let (loss, refl) = slab_transmission_reflection(p, f, lambda / 4.0);
        assert!(loss.abs() < 1e-6 && refl < 1e-6, "loss {loss} refl {refl}");
        // Quarter-wave slab (d = lambda0/(4n)) reflects with |R| = (n^2-1)/(n^2+1) = 0.6.
        let (_, refl_q) = slab_transmission_reflection(p, f, lambda / 8.0);
        assert!((refl_q - 0.6).abs() < 1e-9, "{refl_q}");
    }

    #[test]
    fn invalid_catalogs_are_rejected() {
        let c = MaterialCatalog::builtin().unwrap();
        let mut dup = c.clone();
        dup.materials.push(dup.materials[0].clone());
        assert!(matches!(MaterialCatalog::from_json(&serde_json::to_string(&dup).unwrap()), Err(CatalogError::DuplicateId(_))));
        let mut bad = c;
        bad.materials[0].reflection.ghz5 = 1.5;
        assert!(matches!(MaterialCatalog::from_json(&serde_json::to_string(&bad).unwrap()), Err(CatalogError::InvalidMaterial(_))));
        assert!(MaterialCatalog::from_json("{").is_err());
    }

    #[test]
    fn channel_centers_follow_ieee_numbering() {
        assert_eq!(Band::Ghz2_4.channel_center_mhz(1), 2412.0);
        assert_eq!(Band::Ghz2_4.channel_center_mhz(6), 2437.0);
        assert_eq!(Band::Ghz2_4.channel_center_mhz(14), 2484.0);
        assert_eq!(Band::Ghz5.channel_center_mhz(36), 5180.0);
        assert_eq!(Band::Ghz6.channel_center_mhz(1), 5955.0);
    }

    #[test]
    fn custom_material_validity() {
        assert!(Material::custom("x", "X", MaterialCategory::Wall, 6.0, 0.3).is_valid());
        assert!(!Material::custom("x", "X", MaterialCategory::Wall, -1.0, 0.3).is_valid());
        assert!(!Material::custom("x", "X", MaterialCategory::Wall, f64::NAN, 0.3).is_valid());
    }
}
