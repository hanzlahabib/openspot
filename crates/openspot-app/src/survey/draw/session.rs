//! Editing state machine of the drawing tools. Pure: every operation takes the drawing and
//! reports whether it changed, so the caller knows when to re-render the plan.

use openspot_model::{Drawing, FontSize, LineThickness, Pt, Shape, ShapeKind, Uuid};

use super::hit::top_hit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawTool {
    #[default]
    Select,
    Polyline,
    Rect,
    Text,
}

/// A text shape awaiting its content (new at `at`, or an existing one being edited).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingText {
    pub at: Pt,
    pub text: String,
    pub editing: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    id: Uuid,
    start: Pt,
    now: Pt,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrawSession {
    pub tool: DrawTool,
    pub thickness: LineThickness,
    pub font: FontSize,
    /// Fill new rectangles instead of outlining them.
    pub fill_rects: bool,
    /// Clicks of the polyline or rectangle being drawn.
    pub draft: Vec<Pt>,
    pub selected: Option<Uuid>,
    pub pending_text: Option<PendingText>,
    drag: Option<Drag>,
}

impl DrawSession {
    pub fn set_tool(&mut self, tool: DrawTool) {
        if self.tool != tool {
            self.tool = tool;
            self.cancel();
        }
    }

    /// Abandons whatever is in progress (not the selection).
    pub fn cancel(&mut self) {
        self.draft.clear();
        self.pending_text = None;
        self.drag = None;
    }

    /// Offset the dragged shape would move by, for the ghost preview.
    pub fn drag_offset(&self) -> Option<(Uuid, Pt)> {
        self.drag.map(|d| (d.id, (d.now.0 - d.start.0, d.now.1 - d.start.1)))
    }

    /// Feeds a map click. Returns whether the drawing changed.
    pub fn click(&mut self, d: &mut Drawing, p: Pt, tolerance: f64) -> bool {
        match self.tool {
            DrawTool::Select => {
                self.selected = top_hit(d, p, tolerance);
                false
            }
            DrawTool::Polyline => {
                self.draft.push(p);
                false
            }
            DrawTool::Rect => match self.draft.first().copied() {
                None => {
                    self.draft.push(p);
                    false
                }
                Some(first) => {
                    self.draft.clear();
                    let (min, max) = ((first.0.min(p.0), first.1.min(p.1)), (first.0.max(p.0), first.1.max(p.1)));
                    if min == max {
                        return false;
                    }
                    let shape = Shape::new(ShapeKind::Rect { min, max, filled: self.fill_rects }, self.thickness, self.font);
                    self.selected = Some(shape.id);
                    d.shapes.push(shape);
                    true
                }
            },
            DrawTool::Text => {
                self.pending_text = Some(PendingText { at: p, text: String::new(), editing: None });
                false
            }
        }
    }

    /// Finishes the polyline being drawn; needs two or more points.
    pub fn finish_polyline(&mut self, d: &mut Drawing) -> bool {
        if self.tool != DrawTool::Polyline || self.draft.len() < 2 {
            return false;
        }
        let shape = Shape::new(ShapeKind::Polyline { points: std::mem::take(&mut self.draft) }, self.thickness, self.font);
        self.selected = Some(shape.id);
        d.shapes.push(shape);
        true
    }

    /// Confirms the pending text. Blank text is discarded (or, when editing, deletes
    /// nothing: the old text stays).
    pub fn commit_text(&mut self, d: &mut Drawing) -> bool {
        let Some(pending) = self.pending_text.take() else { return false };
        let text = pending.text.trim_end().to_owned();
        if text.trim().is_empty() {
            return false;
        }
        match pending.editing {
            Some(id) => match d.get_mut(id).map(|s| &mut s.kind) {
                Some(ShapeKind::Text { text: old, .. }) => {
                    let changed = *old != text;
                    *old = text;
                    changed
                }
                _ => false,
            },
            None => {
                let shape = Shape::new(ShapeKind::Text { at: pending.at, text }, self.thickness, self.font);
                self.selected = Some(shape.id);
                d.shapes.push(shape);
                true
            }
        }
    }

    /// Opens the selected text shape for editing.
    pub fn edit_selected_text(&mut self, d: &Drawing) -> bool {
        let Some(shape) = self.selected.and_then(|id| d.get(id)) else { return false };
        let ShapeKind::Text { at, text } = &shape.kind else { return false };
        self.pending_text = Some(PendingText { at: *at, text: text.clone(), editing: Some(shape.id) });
        true
    }

    /// Starts moving the shape under `p` (selecting it) in Select mode.
    pub fn drag_begin(&mut self, d: &Drawing, p: Pt, tolerance: f64) -> bool {
        if self.tool != DrawTool::Select {
            return false;
        }
        let Some(id) = top_hit(d, p, tolerance) else { return false };
        self.selected = Some(id);
        self.drag = Some(Drag { id, start: p, now: p });
        true
    }

    pub fn drag_update(&mut self, p: Pt) {
        if let Some(d) = &mut self.drag {
            d.now = p;
        }
    }

    /// Drops the shape at the pointer. Returns whether it actually moved.
    pub fn drag_end(&mut self, d: &mut Drawing) -> bool {
        let Some(drag) = self.drag.take() else { return false };
        let (dx, dy) = (drag.now.0 - drag.start.0, drag.now.1 - drag.start.1);
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let Some(slot) = d.shapes.iter_mut().find(|s| s.id == drag.id) else { return false };
        *slot = slot.translated(dx, dy);
        true
    }

    pub fn delete_selected(&mut self, d: &mut Drawing) -> bool {
        match self.selected.take() {
            Some(id) => d.remove(id),
            None => false,
        }
    }

    /// Applies the current thickness and font presets to the selected shape.
    pub fn restyle_selected(&mut self, d: &mut Drawing) -> bool {
        let (thickness, font) = (self.thickness, self.font);
        let Some(shape) = self.selected.and_then(|id| d.get_mut(id)) else { return false };
        let changed = shape.thickness != thickness || shape.font != font;
        (shape.thickness, shape.font) = (thickness, font);
        changed
    }
}
