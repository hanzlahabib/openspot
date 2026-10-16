//! Drawing and editing tools as a pure state machine.
//!
//! The canvas turns mouse input into [`Pointer`] events in plan meters; [`Editor`] answers
//! with the [`Edit`]s to apply and keeps the transient state (a half-drawn wall, a drag in
//! progress) plus a [`Preview`] for the canvas to draw. Nothing here knows about egui.
//!
//! Callers must apply the returned edits to the project before sending the next event, so
//! `Context::data` is always current (chained walls rely on it).

use std::borrow::Cow;

use openspot_model::{Cutout, OpeningKind, PlanningData, Point, Uuid, VirtualAp, Wall, WallKind};

use super::ap_models::{ApModel, next_ap_name};
use super::edit::{Change, Edit};
use super::geometry::{End, Hit, MIN_WALL_LENGTH_M, SnapKind, SnapSettings, closest_on_segment, distance, hit_test, snap};
use super::ops::{PlaceError, Selection, moved_opening, nearest_wall, opening_span, place_opening, removal_edit};

/// Drawn depth of a rack or shelf run.
pub const SHELF_THICKNESS_M: f64 = 0.4;
/// Thickest wall the thickness presets and stored preferences may carry.
pub const MAX_THICKNESS_M: f64 = 1.0;
pub const DEFAULT_DOOR_WIDTH_M: f64 = 0.9;
pub const DEFAULT_WINDOW_WIDTH_M: f64 = 1.2;
/// Wall thickness presets offered in the toolbar: label and meters.
pub const THICKNESS_PRESETS_M: [(&str, f64); 4] = [("5 cm", 0.05), ("10 cm", 0.10), ("20 cm", 0.20), ("30 cm", 0.30)];
/// A cutout needs a surface.
pub const MIN_CUTOUT_VERTICES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Wall,
    Shelf,
    Door,
    Window,
    Cutout,
    Ap,
    Delete,
}

impl Tool {
    pub const ALL: [Self; 8] = [Self::Select, Self::Wall, Self::Shelf, Self::Door, Self::Window, Self::Cutout, Self::Ap, Self::Delete];

    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Wall => "Wall",
            Self::Shelf => "Rack",
            Self::Door => "Door",
            Self::Window => "Window",
            Self::Cutout => "Stairwell",
            Self::Ap => "Access point",
            Self::Delete => "Delete",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Select => "Click to select, drag to move; drag a wall end to reshape. Delete removes the selection.",
            Self::Wall => "Drag to draw a wall, or click points to chain walls. Esc or right-click ends.",
            Self::Shelf => "Drag to draw a rack or shelf run. Esc or right-click ends.",
            Self::Door => "Click a wall to set a door into it.",
            Self::Window => "Click a wall to set a window into it.",
            Self::Cutout => "Click the corners of an opening through the floor above; click the first corner or press Enter to finish.",
            Self::Ap => "Click to place the access point chosen in the side panel.",
            Self::Delete => "Click an element to delete it.",
        }
    }
}

/// The user's current drawing defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSettings {
    pub snap: SnapSettings,
    pub thickness_m: f64,
    pub wall_material: String,
    pub shelf_material: String,
    pub door_material: String,
    pub window_material: String,
    pub door_width_m: f64,
    pub window_width_m: f64,
    /// Model new access points are created from.
    pub ap_model: ApModel,
}

impl ToolSettings {
    pub fn new(ap_model: ApModel) -> Self {
        Self {
            snap: SnapSettings::default(),
            thickness_m: THICKNESS_PRESETS_M[1].1,
            wall_material: "drywall".into(),
            shelf_material: "chipboard-shelf".into(),
            door_material: "wood-door".into(),
            window_material: "glass-window".into(),
            door_width_m: DEFAULT_DOOR_WIDTH_M,
            window_width_m: DEFAULT_WINDOW_WIDTH_M,
            ap_model,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// Moved with no button down.
    Hover,
    Press,
    /// Moved with the primary button down.
    Drag,
    Release,
}

/// What the tools need to know about the world for one event.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub zone: Uuid,
    pub data: &'a PlanningData,
    pub settings: &'a ToolSettings,
    /// Pick and snap distance in meters (a fixed number of screen pixels at the current zoom).
    pub tol_m: f64,
}

/// What an event asks the caller to do.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub edits: Vec<Edit>,
    /// Something the user should read (why an action was refused).
    pub message: Option<String>,
}

impl Outcome {
    fn edit(edit: Edit) -> Self {
        Self { edits: vec![edit], message: None }
    }

    fn say(message: impl Into<String>) -> Self {
        Self { edits: Vec::new(), message: Some(message.into()) }
    }
}

/// What the canvas should draw on top of the plan for the tool in use.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Preview {
    #[default]
    None,
    Cursor { at: Point, kind: SnapKind },
    Segment { a: Point, b: Point, thickness_m: f64 },
    Polygon { points: Vec<Point>, cursor: Option<Point>, can_close: bool },
    Opening { wall: Uuid, start: f64, end: f64, ok: bool },
    Ap(Point),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Button held since the start point was pressed.
    Dragging,
    /// Start point set by a click; the next click ends the segment and starts the next one.
    Armed,
}

#[derive(Debug, Clone, PartialEq, Default)]
enum Draft {
    #[default]
    Idle,
    Segment { anchor: Point, phase: Phase },
    Polygon(Vec<Point>),
}

#[derive(Debug, Clone, PartialEq)]
enum Element {
    Wall(Wall),
    Ap(VirtualAp),
    Cutout(Cutout),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Grab {
    /// Pointer minus AP position at press time, so the AP does not jump to the cursor.
    Ap(Point),
    WallEnd(End),
    WallBody(Point),
    /// Opening id and pointer position along the wall minus the opening's center.
    Opening(Uuid, f64),
    CutoutVertex(usize),
    CutoutBody(Point),
}

#[derive(Debug, Clone, PartialEq)]
struct Moving {
    original: Element,
    current: Element,
    grab: Grab,
}

#[derive(Debug, Default)]
pub struct Editor {
    tool: Tool,
    pub selection: Option<Selection>,
    draft: Draft,
    moving: Option<Moving>,
    preview: Preview,
}

fn grid_only(s: &SnapSettings) -> SnapSettings {
    SnapSettings { endpoints: false, angle: false, ..*s }
}

fn offset(p: Point, by: Point) -> Point {
    Point::new(p.x + by.x, p.y + by.y)
}

fn minus(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

impl Editor {
    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    /// True while an element is being dragged.
    pub fn is_grabbing(&self) -> bool {
        self.moving.is_some()
    }

    /// True while a multi-click action (a wall chain, a cutout outline) is in progress.
    pub fn is_drafting(&self) -> bool {
        self.draft != Draft::Idle
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.cancel();
        self.tool = tool;
    }

    /// Esc, right-click or a tool switch: drop whatever is half done.
    pub fn cancel(&mut self) {
        self.draft = Draft::Idle;
        self.moving = None;
        self.preview = Preview::None;
    }

    /// Forgets a selection whose element no longer exists (after undo or deletion).
    pub fn refresh_selection(&mut self, data: &PlanningData) {
        if self.selection.is_some_and(|s| !s.exists_in(data)) {
            self.selection = None;
        }
    }

    pub fn pointer(&mut self, kind: Pointer, pos: Point, ctx: &Context<'_>) -> Outcome {
        match self.tool {
            Tool::Wall | Tool::Shelf => self.segment(kind, pos, ctx),
            Tool::Cutout => self.polygon(kind, pos, ctx),
            Tool::Door | Tool::Window => self.opening(kind, pos, ctx),
            Tool::Ap => self.place_ap(kind, pos, ctx),
            Tool::Delete => self.delete_at(kind, pos, ctx),
            Tool::Select => self.select(kind, pos, ctx),
        }
    }

    /// Enter: finishes a cutout outline.
    pub fn confirm(&mut self, ctx: &Context<'_>) -> Outcome {
        match std::mem::take(&mut self.draft) {
            Draft::Polygon(points) => self.finish_polygon(points, ctx),
            other => {
                self.draft = other;
                Outcome::default()
            }
        }
    }

    /// Delete key: removes the selected element.
    pub fn delete_selection(&mut self, ctx: &Context<'_>) -> Outcome {
        let Some(sel) = self.selection.take() else { return Outcome::default() };
        self.moving = None;
        removal_edit(ctx.zone, ctx.data, sel).map_or_else(Outcome::default, Outcome::edit)
    }

    /// The plan as it looks mid-drag, so the canvas shows the element where it will land.
    pub fn preview_data<'a>(&self, data: &'a PlanningData) -> Cow<'a, PlanningData> {
        let Some(m) = self.moving.as_ref().filter(|m| m.current != m.original) else { return Cow::Borrowed(data) };
        let mut out = data.clone();
        match &m.current {
            Element::Wall(w) => out.walls.iter_mut().filter(|x| x.id == w.id).for_each(|x| *x = w.clone()),
            Element::Ap(a) => out.access_points.iter_mut().filter(|x| x.id == a.id).for_each(|x| *x = a.clone()),
            Element::Cutout(c) => out.cutouts.iter_mut().filter(|x| x.id == c.id).for_each(|x| *x = c.clone()),
        }
        Cow::Owned(out)
    }

    // ---- walls and racks -------------------------------------------------------------

    fn segment(&mut self, kind: Pointer, pos: Point, ctx: &Context<'_>) -> Outcome {
        let (wall_kind, thickness, material) = match self.tool {
            Tool::Shelf => (WallKind::Shelf, SHELF_THICKNESS_M, ctx.settings.shelf_material.clone()),
            _ => (WallKind::Wall, ctx.settings.thickness_m, ctx.settings.wall_material.clone()),
        };
        let draft = self.draft.clone();
        let anchor = match draft {
            Draft::Segment { anchor, .. } => Some(anchor),
            _ => None,
        };
        let s = snap(pos, anchor, ctx.data, &ctx.settings.snap, ctx.tol_m, None);
        let long_enough = |a: Point| distance(a, s.point) >= MIN_WALL_LENGTH_M;
        let commit = |editor: &mut Self, a: Point| {
            let mut wall = Wall::new(a, s.point, wall_kind, material.clone());
            wall.thickness_m = thickness;
            editor.selection = Some(Selection::Wall(wall.id));
            Outcome::edit(Edit::Wall { zone: ctx.zone, change: Change::added(ctx.data.walls.len(), wall) })
        };
        match (kind, draft) {
            (Pointer::Hover | Pointer::Drag | Pointer::Release, Draft::Idle) => {
                self.preview = Preview::Cursor { at: s.point, kind: s.kind };
                Outcome::default()
            }
            (Pointer::Press, Draft::Idle) => {
                self.draft = Draft::Segment { anchor: s.point, phase: Phase::Dragging };
                self.preview = Preview::Segment { a: s.point, b: s.point, thickness_m: thickness };
                Outcome::default()
            }
            (Pointer::Hover | Pointer::Drag, Draft::Segment { anchor, .. }) => {
                self.preview = Preview::Segment { a: anchor, b: s.point, thickness_m: thickness };
                Outcome::default()
            }
            (Pointer::Release, Draft::Segment { anchor, phase: Phase::Dragging }) => {
                if long_enough(anchor) {
                    self.draft = Draft::Idle;
                    self.preview = Preview::Cursor { at: s.point, kind: s.kind };
                    commit(self, anchor)
                } else {
                    // A click, not a drag: wait for the second click.
                    self.draft = Draft::Segment { anchor, phase: Phase::Armed };
                    Outcome::default()
                }
            }
            (Pointer::Press, Draft::Segment { anchor, phase: Phase::Armed }) => {
                if !long_enough(anchor) {
                    return Outcome::default();
                }
                self.draft = Draft::Segment { anchor: s.point, phase: Phase::Armed };
                self.preview = Preview::Segment { a: s.point, b: s.point, thickness_m: thickness };
                commit(self, anchor)
            }
            // The release that ends a click already handled on press, or a stray event.
            _ => Outcome::default(),
        }
    }

    // ---- stairwell outlines ----------------------------------------------------------

    fn polygon(&mut self, kind: Pointer, pos: Point, ctx: &Context<'_>) -> Outcome {
        let points = match &self.draft {
            Draft::Polygon(p) => p.clone(),
            _ => Vec::new(),
        };
        let s = snap(pos, points.last().copied(), ctx.data, &ctx.settings.snap, ctx.tol_m, None);
        match kind {
            Pointer::Press => {
                let closes = points.len() >= MIN_CUTOUT_VERTICES && distance(pos, points[0]) <= ctx.tol_m;
                if closes {
                    self.draft = Draft::Idle;
                    return self.finish_polygon(points, ctx);
                }
                let mut points = points;
                if points.last().is_none_or(|last| distance(*last, s.point) > 0.0) {
                    points.push(s.point);
                }
                self.preview = Preview::Polygon { points: points.clone(), cursor: None, can_close: points.len() >= MIN_CUTOUT_VERTICES };
                self.draft = Draft::Polygon(points);
            }
            Pointer::Hover | Pointer::Drag => {
                self.preview = if points.is_empty() {
                    Preview::Cursor { at: s.point, kind: s.kind }
                } else {
                    Preview::Polygon { can_close: points.len() >= MIN_CUTOUT_VERTICES, points, cursor: Some(s.point) }
                };
            }
            Pointer::Release => {}
        }
        Outcome::default()
    }

    fn finish_polygon(&mut self, points: Vec<Point>, ctx: &Context<'_>) -> Outcome {
        self.preview = Preview::None;
        if points.len() < MIN_CUTOUT_VERTICES {
            return Outcome::say(format!("a stairwell needs at least {MIN_CUTOUT_VERTICES} corners"));
        }
        let cutout = Cutout { id: Uuid::new_v4(), polygon: points };
        self.selection = Some(Selection::Cutout(cutout.id));
        Outcome::edit(Edit::Cutout { zone: ctx.zone, change: Change::added(ctx.data.cutouts.len(), cutout) })
    }

    // ---- doors and windows -----------------------------------------------------------

    fn opening(&mut self, kind: Pointer, pos: Point, ctx: &Context<'_>) -> Outcome {
        let (okind, width, material) = match self.tool {
            Tool::Window => (OpeningKind::Window, ctx.settings.window_width_m, ctx.settings.window_material.as_str()),
            _ => (OpeningKind::Door, ctx.settings.door_width_m, ctx.settings.door_material.as_str()),
        };
        let Some((wall, u)) = nearest_wall(ctx.data, pos, ctx.tol_m) else {
            self.preview = Preview::Cursor { at: pos, kind: SnapKind::None };
            return if kind == Pointer::Press { Outcome::say(PlaceError::NoWall.to_string()) } else { Outcome::default() };
        };
        let placed = place_opening(wall, u, okind, width, material);
        self.preview = match opening_span(wall, u, width) {
            Some((start, end)) => Preview::Opening { wall: wall.id, start, end, ok: placed.is_ok() },
            None => Preview::Cursor { at: pos, kind: SnapKind::None },
        };
        if kind != Pointer::Press {
            return Outcome::default();
        }
        match placed {
            Ok(opening) => {
                let (wall_id, opening_id) = (wall.id, opening.id);
                let index = ctx.data.walls.iter().position(|w| w.id == wall_id);
                let Some(index) = index else { return Outcome::default() };
                let before = ctx.data.walls[index].clone();
                let mut after = before.clone();
                after.openings.push(opening);
                after.openings.sort_by(|a, b| a.start.total_cmp(&b.start));
                self.selection = Some(Selection::Opening { wall: wall_id, opening: opening_id });
                Outcome::edit(Edit::Wall { zone: ctx.zone, change: Change::modified(index, before, after) })
            }
            Err(e) => Outcome::say(e.to_string()),
        }
    }

    // ---- access points ---------------------------------------------------------------

    fn place_ap(&mut self, kind: Pointer, pos: Point, ctx: &Context<'_>) -> Outcome {
        let at = snap(pos, None, ctx.data, &grid_only(&ctx.settings.snap), ctx.tol_m, None).point;
        self.preview = Preview::Ap(at);
        if kind != Pointer::Press {
            return Outcome::default();
        }
        let ap = ctx.settings.ap_model.instantiate(next_ap_name(ctx.data), at);
        self.selection = Some(Selection::Ap(ap.id));
        Outcome::edit(Edit::Ap { zone: ctx.zone, change: Change::added(ctx.data.access_points.len(), ap) })
    }

    // ---- delete tool -----------------------------------------------------------------

    fn delete_at(&mut self, kind: Pointer, pos: Point, ctx: &Context<'_>) -> Outcome {
        if kind != Pointer::Press {
            return Outcome::default();
        }
        let Some(hit) = hit_test(ctx.data, pos, ctx.tol_m) else { return Outcome::default() };
        let target = Selection::from_hit(hit);
        if self.selection == Some(target) {
            self.selection = None;
        }
        removal_edit(ctx.zone, ctx.data, target).map_or_else(Outcome::default, Outcome::edit)
    }

    // ---- select and move -------------------------------------------------------------

    fn select(&mut self, kind: Pointer, pos: Point, ctx: &Context<'_>) -> Outcome {
        match kind {
            Pointer::Hover => Outcome::default(),
            Pointer::Press => {
                self.begin_grab(pos, ctx);
                Outcome::default()
            }
            Pointer::Drag => {
                if let Some(m) = self.moving.take() {
                    let current = moved(&m, pos, ctx);
                    self.moving = Some(Moving { current, ..m });
                }
                Outcome::default()
            }
            Pointer::Release => {
                let Some(m) = self.moving.take() else { return Outcome::default() };
                if m.current == m.original {
                    return Outcome::default();
                }
                commit_move(&m, ctx)
            }
        }
    }

    fn begin_grab(&mut self, pos: Point, ctx: &Context<'_>) {
        self.moving = None;
        let Some(hit) = hit_test(ctx.data, pos, ctx.tol_m) else {
            self.selection = None;
            return;
        };
        self.selection = Some(Selection::from_hit(hit));
        let data = ctx.data;
        let moving = match hit {
            Hit::Ap(id) => data.access_points.iter().find(|a| a.id == id).map(|a| (Element::Ap(a.clone()), Grab::Ap(minus(pos, a.position)))),
            Hit::WallEnd { wall, end } => data.walls.iter().find(|w| w.id == wall).map(|w| (Element::Wall(w.clone()), Grab::WallEnd(end))),
            Hit::Wall { wall, .. } => data.walls.iter().find(|w| w.id == wall).map(|w| (Element::Wall(w.clone()), Grab::WallBody(pos))),
            Hit::Opening { wall, opening, u } => data.walls.iter().find(|w| w.id == wall).and_then(|w| {
                let op = w.openings.iter().find(|o| o.id == opening)?;
                Some((Element::Wall(w.clone()), Grab::Opening(opening, u - (op.start + op.end) / 2.0)))
            }),
            Hit::CutoutVertex { cutout, index } => data.cutouts.iter().find(|c| c.id == cutout).map(|c| (Element::Cutout(c.clone()), Grab::CutoutVertex(index))),
            Hit::Cutout(id) => data.cutouts.iter().find(|c| c.id == id).map(|c| (Element::Cutout(c.clone()), Grab::CutoutBody(pos))),
        };
        self.moving = moving.map(|(original, grab)| Moving { current: original.clone(), original, grab });
    }
}

/// The dragged element with the pointer at `pos`.
fn moved(m: &Moving, pos: Point, ctx: &Context<'_>) -> Element {
    let s = &ctx.settings.snap;
    let data = ctx.data;
    match (&m.original, m.grab) {
        (Element::Ap(ap), Grab::Ap(grab)) => {
            let mut ap = ap.clone();
            ap.position = snap(minus(pos, grab), None, data, &grid_only(s), ctx.tol_m, None).point;
            Element::Ap(ap)
        }
        (Element::Wall(w), Grab::WallEnd(end)) => {
            let mut w = w.clone();
            let fixed = if end == End::A { w.b } else { w.a };
            let p = snap(pos, Some(fixed), data, s, ctx.tol_m, Some(w.id)).point;
            if end == End::A { w.a = p } else { w.b = p }
            Element::Wall(w)
        }
        (Element::Wall(w), Grab::WallBody(start)) => {
            let mut w = w.clone();
            let new_a = snap(offset(w.a, minus(pos, start)), None, data, &grid_only(s), ctx.tol_m, None).point;
            let shift = minus(new_a, w.a);
            (w.a, w.b) = (new_a, offset(w.b, shift));
            Element::Wall(w)
        }
        (Element::Wall(w), Grab::Opening(id, grab_u)) => {
            let u = closest_on_segment(pos, w.a, w.b).u;
            // An overlap keeps the previous position instead of jumping over a neighbour.
            let current = match &m.current {
                Element::Wall(c) => c,
                _ => w,
            };
            Element::Wall(moved_opening(current, id, u - grab_u).unwrap_or_else(|| current.clone()))
        }
        (Element::Cutout(c), Grab::CutoutVertex(i)) => {
            let mut c = c.clone();
            if let Some(v) = c.polygon.get_mut(i) {
                *v = snap(pos, None, data, s, ctx.tol_m, None).point;
            }
            Element::Cutout(c)
        }
        (Element::Cutout(c), Grab::CutoutBody(start)) => {
            let mut c = c.clone();
            if let Some(first) = c.polygon.first().copied() {
                let new_first = snap(offset(first, minus(pos, start)), None, data, &grid_only(s), ctx.tol_m, None).point;
                let shift = minus(new_first, first);
                c.polygon.iter_mut().for_each(|v| *v = offset(*v, shift));
            }
            Element::Cutout(c)
        }
        _ => m.current.clone(),
    }
}

fn commit_move(m: &Moving, ctx: &Context<'_>) -> Outcome {
    let data = ctx.data;
    match (&m.original, &m.current) {
        (Element::Wall(before), Element::Wall(after)) => {
            if after.length_m() < MIN_WALL_LENGTH_M {
                return Outcome::say("that would collapse the wall");
            }
            let Some(index) = data.walls.iter().position(|w| w.id == before.id) else { return Outcome::default() };
            Outcome::edit(Edit::Wall { zone: ctx.zone, change: Change::modified(index, before.clone(), after.clone()) })
        }
        (Element::Ap(before), Element::Ap(after)) => {
            let Some(index) = data.access_points.iter().position(|a| a.id == before.id) else { return Outcome::default() };
            Outcome::edit(Edit::Ap { zone: ctx.zone, change: Change::modified(index, before.clone(), after.clone()) })
        }
        (Element::Cutout(before), Element::Cutout(after)) => {
            let Some(index) = data.cutouts.iter().position(|c| c.id == before.id) else { return Outcome::default() };
            Outcome::edit(Edit::Cutout { zone: ctx.zone, change: Change::modified(index, before.clone(), after.clone()) })
        }
        _ => Outcome::default(),
    }
}

#[cfg(test)]
mod tests;
