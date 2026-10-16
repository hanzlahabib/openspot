//! Element-level operations that produce [`Edit`]s: selection, removal, modification and
//! opening placement. Pure, so the tools and the property panels share one tested path.

use openspot_model::{Opening, OpeningKind, PlanningData, Uuid, VirtualAp, Wall};

use super::edit::{Change, Edit};
use super::geometry::{Hit, closest_on_segment};
use openspot_model::Point;

/// The element a user has selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Wall(Uuid),
    Opening { wall: Uuid, opening: Uuid },
    Ap(Uuid),
    Cutout(Uuid),
}

impl Selection {
    /// What a click on `hit` selects: a wall end or cutout vertex selects its whole element.
    pub fn from_hit(hit: Hit) -> Self {
        match hit {
            Hit::Ap(id) => Self::Ap(id),
            Hit::WallEnd { wall, .. } | Hit::Wall { wall, .. } => Self::Wall(wall),
            Hit::Opening { wall, opening, .. } => Self::Opening { wall, opening },
            Hit::CutoutVertex { cutout, .. } | Hit::Cutout(cutout) => Self::Cutout(cutout),
        }
    }

    /// Whether the selected element is still present in `data`.
    pub fn exists_in(self, data: &PlanningData) -> bool {
        match self {
            Self::Wall(id) => data.walls.iter().any(|w| w.id == id),
            Self::Opening { wall, opening } => data.walls.iter().any(|w| w.id == wall && w.openings.iter().any(|o| o.id == opening)),
            Self::Ap(id) => data.access_points.iter().any(|a| a.id == id),
            Self::Cutout(id) => data.cutouts.iter().any(|c| c.id == id),
        }
    }
}

/// Edit that deletes the selected element; deleting an opening leaves its wall.
pub fn removal_edit(zone: Uuid, data: &PlanningData, target: Selection) -> Option<Edit> {
    match target {
        Selection::Wall(id) => {
            let index = data.walls.iter().position(|w| w.id == id)?;
            Some(Edit::Wall { zone, change: Change::removed(index, data.walls[index].clone()) })
        }
        Selection::Opening { wall, opening } => modify_wall(zone, data, wall, |w| w.openings.retain(|o| o.id != opening)),
        Selection::Ap(id) => {
            let index = data.access_points.iter().position(|a| a.id == id)?;
            Some(Edit::Ap { zone, change: Change::removed(index, data.access_points[index].clone()) })
        }
        Selection::Cutout(id) => {
            let index = data.cutouts.iter().position(|c| c.id == id)?;
            Some(Edit::Cutout { zone, change: Change::removed(index, data.cutouts[index].clone()) })
        }
    }
}

/// Edit that applies `f` to wall `id`; `None` if the wall is missing or `f` changed nothing.
pub fn modify_wall(zone: Uuid, data: &PlanningData, id: Uuid, f: impl FnOnce(&mut Wall)) -> Option<Edit> {
    let index = data.walls.iter().position(|w| w.id == id)?;
    let before = data.walls[index].clone();
    let mut after = before.clone();
    f(&mut after);
    (after != before).then(|| Edit::Wall { zone, change: Change::modified(index, before, after) })
}

pub fn modify_ap(zone: Uuid, data: &PlanningData, id: Uuid, f: impl FnOnce(&mut VirtualAp)) -> Option<Edit> {
    let index = data.access_points.iter().position(|a| a.id == id)?;
    let before = data.access_points[index].clone();
    let mut after = before.clone();
    f(&mut after);
    (after != before).then(|| Edit::Ap { zone, change: Change::modified(index, before, after) })
}

/// Edit that sets the material of the slab under this level; `None` if unchanged.
pub fn slab_material_edit(zone: Uuid, data: &PlanningData, material: &str) -> Option<Edit> {
    let before = data.level.slab.clone();
    let mut after = before.clone();
    after.material = material.to_owned();
    (after != before).then_some(Edit::Slab { zone, before, after })
}

/// Why an opening could not be placed on a wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlaceError {
    #[error("click on a wall to place it")]
    NoWall,
    #[error("the wall is shorter than the opening")]
    TooShort,
    #[error("it would overlap another door or window")]
    Overlaps,
}

/// Fractions `(start, end)` of an opening `width_m` wide centered at wall position `u`,
/// shifted to stay on the wall. `None` if the wall is shorter than the opening.
pub fn opening_span(wall: &Wall, u: f64, width_m: f64) -> Option<(f64, f64)> {
    let length = wall.length_m();
    if length <= 0.0 || width_m > length {
        return None;
    }
    let span = width_m / length;
    let start = (u - span / 2.0).clamp(0.0, 1.0 - span);
    Some((start, start + span))
}

fn overlaps(wall: &Wall, start: f64, end: f64, except: Option<Uuid>) -> bool {
    wall.openings.iter().filter(|o| Some(o.id) != except).any(|o| start < o.end && o.start < end)
}

/// A door or window of `width_m` centered at wall position `u`.
pub fn place_opening(wall: &Wall, u: f64, kind: OpeningKind, width_m: f64, material: &str) -> Result<Opening, PlaceError> {
    let (start, end) = opening_span(wall, u, width_m).ok_or(PlaceError::TooShort)?;
    if overlaps(wall, start, end, None) {
        return Err(PlaceError::Overlaps);
    }
    Ok(Opening { id: Uuid::new_v4(), kind, start, end, material: material.to_owned() })
}

/// The wall nearest `p` within `tol_m` and where `p` projects onto it.
pub fn nearest_wall(data: &PlanningData, p: Point, tol_m: f64) -> Option<(&Wall, f64)> {
    data.walls
        .iter()
        .map(|w| (w, closest_on_segment(p, w.a, w.b)))
        .filter(|(w, h)| h.distance <= tol_m.max(w.thickness_m / 2.0))
        .min_by(|a, b| a.1.distance.total_cmp(&b.1.distance))
        .map(|(w, h)| (w, h.u))
}

/// Moves opening `id` so its center follows wall position `center_u`, keeping its width and
/// staying on the wall. `None` if that would overlap a neighbour.
pub fn moved_opening(wall: &Wall, id: Uuid, center_u: f64) -> Option<Wall> {
    let op = wall.openings.iter().find(|o| o.id == id)?;
    let half = (op.end - op.start) / 2.0;
    let start = (center_u - half).clamp(0.0, 1.0 - 2.0 * half);
    let end = start + 2.0 * half;
    if overlaps(wall, start, end, Some(id)) {
        return None;
    }
    let mut out = wall.clone();
    if let Some(o) = out.openings.iter_mut().find(|o| o.id == id) {
        (o.start, o.end) = (start, end);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::WallKind;

    fn wall(len: f64) -> Wall {
        Wall::new(Point::new(0.0, 0.0), Point::new(len, 0.0), WallKind::Wall, "drywall")
    }

    #[test]
    fn opening_is_centered_and_kept_on_the_wall() {
        let w = wall(4.0);
        let (s, e) = opening_span(&w, 0.5, 0.9).unwrap();
        assert!((s - (0.5 - 0.1125)).abs() < 1e-12 && (e - (0.5 + 0.1125)).abs() < 1e-12);
        assert!(((e - s) * w.length_m() - 0.9).abs() < 1e-12, "width in meters is the requested one");
        let (s, e) = opening_span(&w, 0.0, 0.9).unwrap();
        assert_eq!(s, 0.0);
        assert!((e - 0.225).abs() < 1e-12, "pushed in from the end");
        let (s, e) = opening_span(&w, 1.0, 0.9).unwrap();
        assert_eq!(e, 1.0);
        assert!((s - 0.775).abs() < 1e-12);
        assert_eq!(opening_span(&wall(0.5), 0.5, 0.9), None);
        assert_eq!(opening_span(&wall(0.9), 0.5, 0.9), Some((0.0, 1.0)), "exactly as wide as the wall");
    }

    #[test]
    fn overlapping_and_too_wide_placements_are_refused() {
        let mut w = wall(4.0);
        let door = place_opening(&w, 0.5, OpeningKind::Door, 0.9, "wood-door").unwrap();
        w.openings.push(door);
        assert_eq!(place_opening(&w, 0.55, OpeningKind::Window, 1.2, "glass-window").unwrap_err(), PlaceError::Overlaps);
        let touching = place_opening(&w, 0.9, OpeningKind::Window, 1.2, "glass-window");
        assert!(touching.is_ok(), "a window further along fits: {touching:?}");
        assert_eq!(place_opening(&wall(1.0), 0.5, OpeningKind::Window, 1.2, "glass-window").unwrap_err(), PlaceError::TooShort);
    }

    #[test]
    fn adjacent_openings_do_not_count_as_overlapping() {
        let mut w = wall(10.0);
        w.openings.push(Opening { id: Uuid::new_v4(), kind: OpeningKind::Door, start: 0.2, end: 0.3, material: "wood-door".into() });
        assert!(!overlaps(&w, 0.3, 0.4, None));
        assert!(overlaps(&w, 0.29, 0.4, None));
    }

    #[test]
    fn moving_an_opening_clamps_and_respects_neighbours() {
        let mut w = wall(10.0);
        let a = Opening { id: Uuid::new_v4(), kind: OpeningKind::Door, start: 0.1, end: 0.2, material: "wood-door".into() };
        let b = Opening { id: Uuid::new_v4(), kind: OpeningKind::Window, start: 0.6, end: 0.8, material: "glass-window".into() };
        let (ida, idb) = (a.id, b.id);
        w.openings = vec![a, b];
        let moved = moved_opening(&w, ida, 0.4).unwrap();
        let o = &moved.openings[0];
        assert!((o.start - 0.35).abs() < 1e-12 && (o.end - 0.45).abs() < 1e-12);
        assert_eq!(moved_opening(&w, ida, -3.0).unwrap().openings[0].start, 0.0);
        assert!(moved_opening(&w, ida, 0.65).is_none(), "would overlap the window");
        assert!((moved_opening(&w, idb, 9.0).unwrap().openings[1].end - 1.0).abs() < 1e-12);
        assert!(moved_opening(&w, Uuid::new_v4(), 0.5).is_none());
    }

    #[test]
    fn removing_an_opening_keeps_the_wall_and_removing_a_wall_takes_its_openings() {
        let zone = Uuid::new_v4();
        let mut data = PlanningData::default();
        let mut w = wall(4.0);
        let door = place_opening(&w, 0.5, OpeningKind::Door, 0.9, "wood-door").unwrap();
        let (wid, did) = (w.id, door.id);
        w.openings.push(door);
        data.walls.push(w);
        let mut project = openspot_model::Project::new("p");
        project.zones[0].id = zone;
        project.zones[0].planning = data.clone();

        let e = removal_edit(zone, &data, Selection::Opening { wall: wid, opening: did }).unwrap();
        assert!(e.apply(&mut project, true));
        assert!(project.zones[0].planning.walls[0].openings.is_empty() && project.zones[0].planning.walls.len() == 1);
        assert!(e.apply(&mut project, false));
        assert_eq!(project.zones[0].planning, data);
        let e = removal_edit(zone, &data, Selection::Wall(wid)).unwrap();
        assert!(e.apply(&mut project, true) && project.zones[0].planning.walls.is_empty());
        assert!(removal_edit(zone, &data, Selection::Wall(Uuid::new_v4())).is_none());
    }

    #[test]
    fn modify_yields_nothing_when_nothing_changed() {
        let zone = Uuid::new_v4();
        let mut data = PlanningData::default();
        let w = wall(4.0);
        let id = w.id;
        data.walls.push(w);
        assert!(modify_wall(zone, &data, id, |_| {}).is_none());
        let e = modify_wall(zone, &data, id, |w| w.thickness_m = 0.3).unwrap();
        assert!(matches!(e, Edit::Wall { change: Change { index: 0, after: Some(ref w), .. }, .. } if w.thickness_m == 0.3));
        assert!(modify_wall(zone, &data, Uuid::new_v4(), |w| w.thickness_m = 0.3).is_none());
    }

    #[test]
    fn ap_and_slab_edits_report_no_change_and_round_trip() {
        let mut project = openspot_model::Project::new("p");
        let zone = project.zones[0].id;
        let ap = VirtualAp::new("a", Point::new(1.0, 1.0), vec![]);
        let id = ap.id;
        project.zones[0].planning.access_points.push(ap);
        let data = project.zones[0].planning.clone();
        assert!(modify_ap(zone, &data, id, |_| {}).is_none() && modify_ap(zone, &data, Uuid::new_v4(), |a| a.height_m = 9.0).is_none());
        let e = modify_ap(zone, &data, id, |a| a.height_m = 9.0).unwrap();
        assert!(e.apply(&mut project, true) && project.zones[0].planning.access_points[0].height_m == 9.0);
        assert!(e.apply(&mut project, false) && project.zones[0].planning.access_points[0].height_m == data.access_points[0].height_m);

        assert!(slab_material_edit(zone, &data, &data.level.slab.material).is_none());
        let e = slab_material_edit(zone, &data, "wood-floor").unwrap();
        assert!(e.apply(&mut project, true) && project.zones[0].planning.level.slab.material == "wood-floor");
        assert!(e.apply(&mut project, false) && project.zones[0].planning.level.slab == data.level.slab);
    }

    #[test]
    fn selection_follows_hits_and_notices_deletion() {
        let mut data = PlanningData::default();
        let w = wall(4.0);
        let id = w.id;
        data.walls.push(w);
        let sel = Selection::from_hit(Hit::WallEnd { wall: id, end: super::super::geometry::End::B });
        assert_eq!(sel, Selection::Wall(id));
        assert!(sel.exists_in(&data));
        data.walls.clear();
        assert!(!sel.exists_in(&data));
        let cid = Uuid::new_v4();
        assert_eq!(Selection::from_hit(Hit::CutoutVertex { cutout: cid, index: 1 }), Selection::Cutout(cid));
    }
}
