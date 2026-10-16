//! Optional single-bounce reflections by the image method (P-08).
//!
//! For every wall on the receiver's level, the transmitter is mirrored across the wall's
//! line; the straight line from that image to the receiver, if it hits the wall segment,
//! is the geometric reflected path (see e.g. Rappaport, *Wireless Communications*, ch. 4,
//! two-ray / image models; ITU-R P.1238 section on ray tracing). Each such path:
//!
//! * has unfolded length `L = |image -> rx|` (3-D with the height difference), paying
//!   the same distance loss as a direct path of that length;
//! * pays `-20 log10(|Gamma|)` dB at the reflection, with `|Gamma|` the wall material's
//!   reflection magnitude for the band;
//! * pays the wall losses on its two legs (the reflecting wall itself is excluded because
//!   the path ends on it);
//! * leaves the antenna toward the reflection point.
//!
//! Contributions are summed as power (non-coherent), so there is no fading: it fills in
//! shadowed corridors and corners but cannot predict interference nulls. Only walls on
//! the receiver's own level reflect; paths between levels are direct only. No double
//! bounces, no reflection off slabs, and no diffraction.

use openspot_model::Point;

use crate::antenna::{Direction, gain_dbi};
use crate::error::PredictError;
use crate::geom::{Intersection, intersect};
use crate::grid::rssi_dbm;
use crate::model::PropagationModel;
use crate::scene::{Position, Scene, Transmitter};

/// Reflection magnitudes at or below this are ignored (a perfectly absorbing surface).
const MIN_REFLECTION: f64 = 1e-3;

fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

fn mirror(p: Point, a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
    let foot = Point::new(a.x + t * dx, a.y + t * dy);
    Point::new(2.0 * foot.x - p.x, 2.0 * foot.y - p.y)
}

fn milliwatts(dbm: f64) -> f64 {
    10f64.powf(dbm / 10.0)
}

/// Received power in dBm at `rx`: the direct path plus every single-bounce reflection.
pub fn rssi_dbm_single_bounce(scene: &Scene, model: &PropagationModel, tx: &Transmitter, rx: Position) -> Result<f64, PredictError> {
    let direct = rssi_dbm(scene, model, tx, rx)?;
    if tx.position.level != rx.level {
        return Ok(direct);
    }
    let (p, z_tx) = scene.world(tx.position)?;
    let (q, z_rx) = scene.world(rx)?;
    let dz = z_rx - z_tx;
    let radio = &tx.radio;
    let (band, freq) = (radio.band, tx.center_frequency_mhz());

    let mut total_mw = milliwatts(direct);
    for wall in &scene.levels[rx.level].walls {
        let gamma = wall.reflection.get(band);
        if gamma < MIN_REFLECTION {
            continue;
        }
        let side = |pt: Point| cross(Point::new(wall.b.x - wall.a.x, wall.b.y - wall.a.y), Point::new(pt.x - wall.a.x, pt.y - wall.a.y));
        let (sp, sq) = (side(p), side(q));
        // Both ends strictly on the same side, otherwise there is no mirror geometry.
        if sp * sq <= 0.0 {
            continue;
        }
        let image = mirror(p, wall.a, wall.b);
        let Intersection::Point { u, .. } = intersect(image, q, wall.a, wall.b) else { continue };
        if wall.openings.iter().any(|(s, e, _)| u >= *s && u <= *e) {
            continue; // glass/door openings are not treated as mirrors
        }
        let hit = Point::new(wall.a.x + u * (wall.b.x - wall.a.x), wall.a.y + u * (wall.b.y - wall.a.y));
        let length_2d = (image.x - q.x).hypot(image.y - q.y);
        let length = length_2d.hypot(dz);
        let (leg1_db, _) = scene.wall_loss(rx.level, p, hit, band);
        let (leg2_db, _) = scene.wall_loss(rx.level, hit, q, band);
        let gain = gain_dbi(radio, Direction::between(hit.x - p.x, hit.y - p.y, dz * ((hit.x - p.x).hypot(hit.y - p.y)) / length_2d));
        let loss = model.distance_loss_db(length, freq) + leg1_db + leg2_db - 20.0 * gamma.log10();
        total_mw += milliwatts(radio.tx_power_dbm + gain - loss);
    }
    Ok(10.0 * total_mw.log10())
}

#[cfg(test)]
mod tests;
