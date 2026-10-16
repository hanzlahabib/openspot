//! A resolved, immutable building ready for propagation queries.
//!
//! Levels are ordered bottom to top. Each wall's material is resolved to per-band losses
//! up front, and coordinates are shifted by the level's alignment offset so that every
//! level shares one world frame. Callers keep using each level's own plan coordinates in
//! [`Position`] and [`Transmitter`]; the scene applies the offset.

use openspot_model::{
    AntennaPattern, Band, MaterialCatalog, PerBand, PlanningData, Point, Radio, Uuid, VirtualAp,
};

use crate::error::PredictError;
use crate::geom::{point_in_polygon, wall_crossing};
use crate::model::PropagationModel;

/// A point in a level: plan coordinates (m) of that level and height above its floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub level: usize,
    pub x: f64,
    pub y: f64,
    pub height_m: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct SceneWall {
    pub(crate) a: Point,
    pub(crate) b: Point,
    pub(crate) loss: PerBand,
    pub(crate) reflection: PerBand,
    /// `(start, end, loss)` for each opening.
    pub(crate) openings: Vec<(f64, f64, PerBand)>,
}

impl SceneWall {
    pub(crate) fn loss_at(&self, u: f64, band: Band) -> f64 {
        self.openings.iter().find(|(s, e, _)| u >= *s && u <= *e).map_or(self.loss.get(band), |(_, _, l)| l.get(band))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SceneLevel {
    pub(crate) elevation_m: f64,
    pub(crate) offset: Point,
    pub(crate) walls: Vec<SceneWall>,
    pub(crate) cutouts: Vec<Vec<Point>>,
    /// Loss of the slab *below* this level (crossed when going from the level beneath).
    pub(crate) slab_loss: PerBand,
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub(crate) levels: Vec<SceneLevel>,
}

/// One enabled radio of a virtual AP, with everything needed to predict its signal.
#[derive(Debug, Clone, PartialEq)]
pub struct Transmitter {
    pub ap_id: Uuid,
    pub radio_index: usize,
    pub radio: Radio,
    pub position: Position,
}

impl Transmitter {
    pub fn center_frequency_mhz(&self) -> f64 {
        self.radio.center_frequency_mhz()
    }
}

/// Itemised loss of one TX -> RX path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathLoss {
    /// Straight-line 3-D distance (m).
    pub distance_m: f64,
    /// Free-space + log-distance + calibration offset.
    pub distance_db: f64,
    pub wall_db: f64,
    pub floor_db: f64,
    pub wall_crossings: u32,
    /// Slabs crossed that were not bypassed through a cutout.
    pub slab_crossings: u32,
}

impl PathLoss {
    pub fn total_db(&self) -> f64 {
        self.distance_db + self.wall_db + self.floor_db
    }
}

impl Scene {
    /// Builds a scene from per-level planning data, ordered bottom to top.
    ///
    /// Fails if a wall/slab references an unknown material, or level elevations are not
    /// strictly increasing (the vertical order is what decides which slabs a path crosses).
    pub fn build(levels: &[&PlanningData], catalog: &MaterialCatalog) -> Result<Self, PredictError> {
        let mut out = Vec::with_capacity(levels.len());
        for (i, data) in levels.iter().enumerate() {
            if i > 0 && data.level.elevation_m <= levels[i - 1].level.elevation_m {
                return Err(PredictError::InvalidInput(format!("level {i} is not above level {}", i - 1)));
            }
            let resolve = |id: &str| {
                data.resolve_material(id, catalog).ok_or_else(|| PredictError::UnknownMaterial(id.to_owned()))
            };
            let off = data.level.offset;
            let shift = |p: Point| Point::new(p.x + off.x, p.y + off.y);
            let mut walls = Vec::with_capacity(data.walls.len());
            for w in &data.walls {
                let m = resolve(&w.material)?;
                let mut openings = Vec::with_capacity(w.openings.len());
                for o in &w.openings {
                    openings.push((o.start, o.end, resolve(&o.material)?.absorption_db));
                }
                walls.push(SceneWall { a: shift(w.a), b: shift(w.b), loss: m.absorption_db, reflection: m.reflection, openings });
            }
            out.push(SceneLevel {
                elevation_m: data.level.elevation_m,
                offset: off,
                walls,
                cutouts: data.cutouts.iter().map(|c| c.polygon.iter().map(|&p| shift(p)).collect()).collect(),
                slab_loss: resolve(&data.level.slab.material)?.absorption_db,
            });
        }
        Ok(Self { levels: out })
    }

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// All enabled radios of the APs on `levels` (bottom to top, same order as `build`),
    /// in a stable order: level, AP, radio.
    pub fn transmitters(levels: &[&PlanningData]) -> Result<Vec<Transmitter>, PredictError> {
        let mut out = Vec::new();
        for (level, data) in levels.iter().enumerate() {
            for ap in &data.access_points {
                out.extend(ap_transmitters(ap, level)?);
            }
        }
        Ok(out)
    }

    /// Summed loss and count of the walls of `level` crossed by the world-frame segment `p -> q`.
    pub(crate) fn wall_loss(&self, level: usize, p: Point, q: Point, band: Band) -> (f64, u32) {
        let mut total = (0.0, 0);
        for w in &self.levels[level].walls {
            if let Some(u) = wall_crossing(p, q, w.a, w.b) {
                total.0 += w.loss_at(u, band);
                total.1 += 1;
            }
        }
        total
    }

    /// World-frame plan position and absolute height of `pos`.
    pub(crate) fn world(&self, pos: Position) -> Result<(Point, f64), PredictError> {
        let level = self.levels.get(pos.level).ok_or_else(|| PredictError::InvalidInput(format!("no level {}", pos.level)))?;
        Ok((Point::new(pos.x + level.offset.x, pos.y + level.offset.y), level.elevation_m + pos.height_m))
    }

    /// Loss from `tx` to `rx` at `freq_mhz` in `band`.
    ///
    /// The straight 3-D segment is split at each slab it crosses. Within each level the
    /// horizontal projection of its part is tested against that level's walls; each slab
    /// crossed adds the slab loss unless the crossing point lies inside a cutout of the
    /// level above the slab. Full-height walls are assumed (no wall tops or sills).
    pub fn path_loss(&self, model: &PropagationModel, tx: Position, rx: Position, freq_mhz: f64, band: Band) -> Result<PathLoss, PredictError> {
        let (p, z0) = self.world(tx)?;
        let (q, z1) = self.world(rx)?;
        let (dx, dy, dz) = (q.x - p.x, q.y - p.y, z1 - z0);
        let distance_m = (dx * dx + dy * dy + dz * dz).sqrt();

        let (mut wall_db, mut floor_db) = (0.0, 0.0);
        let (mut wall_crossings, mut slab_crossings) = (0, 0);
        let at = |t: f64| Point::new(p.x + dx * t, p.y + dy * t);

        // Levels visited from TX to RX; consecutive levels are separated by the slab of the upper one.
        let step: isize = if rx.level >= tx.level { 1 } else { -1 };
        let mut level = tx.level as isize;
        let mut t_start = 0.0f64;
        loop {
            let l = level as usize;
            let last = l == rx.level;
            let boundary_slab = (!last).then(|| if step > 0 { l + 1 } else { l });
            let t_end = match boundary_slab {
                Some(s) => {
                    let t = if dz.abs() > 0.0 { (self.levels[s].elevation_m - z0) / dz } else { 1.0 };
                    t.clamp(t_start, 1.0)
                }
                None => 1.0,
            };
            let (sp, sq) = (at(t_start), at(t_end));
            let (db, n) = self.wall_loss(l, sp, sq, band);
            wall_db += db;
            wall_crossings += n;
            if let Some(s) = boundary_slab {
                let crossing = at(t_end);
                let through_cutout = self.levels[s].cutouts.iter().any(|c| point_in_polygon(crossing, c));
                if !through_cutout {
                    floor_db += self.levels[s].slab_loss.get(band);
                    slab_crossings += 1;
                }
            }
            if last {
                break;
            }
            level += step;
            t_start = t_end;
        }
        Ok(PathLoss { distance_m, distance_db: model.distance_loss_db(distance_m, freq_mhz), wall_db, floor_db, wall_crossings, slab_crossings })
    }
}

fn ap_transmitters(ap: &VirtualAp, level: usize) -> Result<Vec<Transmitter>, PredictError> {
    let mut out = Vec::new();
    for (radio_index, radio) in ap.radios.iter().enumerate().filter(|(_, r)| r.enabled) {
        validate_radio(&ap.name, radio)?;
        out.push(Transmitter {
            ap_id: ap.id,
            radio_index,
            radio: radio.clone(),
            position: Position { level, x: ap.position.x, y: ap.position.y, height_m: ap.height_m },
        });
    }
    Ok(out)
}

fn validate_radio(ap_name: &str, r: &Radio) -> Result<(), PredictError> {
    let bad = |what: &str| PredictError::InvalidInput(format!("AP '{ap_name}': {what}"));
    if !r.tx_power_dbm.is_finite() || !r.antenna_gain_dbi.is_finite() {
        return Err(bad("transmit power and antenna gain must be finite"));
    }
    if r.width_mhz == 0 {
        return Err(bad("channel width must be positive"));
    }
    if let AntennaPattern::Directional { h_beamwidth_deg, v_beamwidth_deg, front_to_back_db, .. } = &r.pattern {
        let ok = *h_beamwidth_deg > 0.0 && *v_beamwidth_deg > 0.0 && *front_to_back_db >= 0.0;
        if !ok || !(h_beamwidth_deg + v_beamwidth_deg + front_to_back_db).is_finite() {
            return Err(bad("directional beamwidths must be positive and front-to-back non-negative"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
