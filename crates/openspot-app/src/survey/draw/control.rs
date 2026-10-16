//! Survey-level drawing controls: creating a drawn plan and keeping the map image in sync.

use openspot_model::{Drawing, MapImage, Scale};

use super::{DRAWN_PLAN_NAME, DrawError, MAX_DRAWING_SIDE_PX, render_png};
use crate::survey::{BLANK_LONG_SIDE_PX, BLANK_MAX_METERS, Survey};

impl Survey {
    pub fn drawing(&self) -> Option<&Drawing> {
        self.project.canvas.zone(self.zone().id).and_then(|z| z.drawing.as_ref())
    }

    /// Forgets the drawing of the current zone (a real floor plan replaces it).
    pub(crate) fn discard_drawing(&mut self) {
        let id = self.zone().id;
        if self.project.canvas.zone(id).is_some_and(|z| z.drawing.is_some()) {
            self.project.canvas.zone_mut(id).drawing = None;
        }
        self.draw.cancel();
        self.draw.selected = None;
    }

    /// Starts an empty drawn plan of the given real size, replacing the zone's plan, and
    /// sets the scale so no calibration is needed.
    pub fn draw_new_plan(&mut self, width_m: f64, height_m: f64) -> Result<(), String> {
        let valid = |v: f64| v.is_finite() && (1.0..=BLANK_MAX_METERS).contains(&v);
        if !valid(width_m) || !valid(height_m) {
            return Err(format!("size must be between 1 and {BLANK_MAX_METERS} meters"));
        }
        let ppm = BLANK_LONG_SIDE_PX / width_m.max(height_m);
        let side = |m: f64| ((m * ppm).round() as u32).clamp(1, MAX_DRAWING_SIDE_PX);
        let drawing = Drawing::new(side(width_m), side(height_m));
        self.apply_drawing(drawing).map_err(|e| e.to_string())?;
        self.zone_mut().scale = Scale::new(ppm);
        self.draw = Default::default();
        self.needs_fit = true;
        self.history.clear();
        Ok(())
    }

    /// Re-renders the zone's map image from `drawing` and stores both.
    fn apply_drawing(&mut self, drawing: Drawing) -> Result<(), DrawError> {
        let data = render_png(&drawing)?;
        let (width, height) = (drawing.width, drawing.height);
        let id = self.zone().id;
        self.project.canvas.zone_mut(id).drawing = Some(drawing);
        self.zone_mut().map = Some(MapImage { original_name: DRAWN_PLAN_NAME.to_owned(), data, width, height });
        self.textures.maps.remove(&id);
        self.touch();
        Ok(())
    }

    /// Runs an edit on the drawing and, if it changed anything, re-renders the plan.
    fn edit_drawing(&mut self, edit: impl FnOnce(&mut super::DrawSession, &mut Drawing) -> bool) {
        let Some(mut drawing) = self.drawing().cloned() else { return };
        if edit(&mut self.draw, &mut drawing) {
            if let Err(e) = self.apply_drawing(drawing) {
                self.error = Some(e.to_string());
            }
        }
    }

    /// Map click with the drawing tool active; `tolerance` is the hit slack in map pixels.
    pub fn draw_click(&mut self, x: f64, y: f64, tolerance: f64) {
        self.edit_drawing(|session, d| session.click(d, (x, y), tolerance));
    }

    pub fn draw_finish_polyline(&mut self) {
        self.edit_drawing(|session, d| session.finish_polyline(d));
    }

    pub fn draw_commit_text(&mut self) {
        self.edit_drawing(|session, d| session.commit_text(d));
    }

    pub fn draw_delete_selected(&mut self) {
        self.edit_drawing(|session, d| session.delete_selected(d));
    }

    pub fn draw_restyle_selected(&mut self) {
        self.edit_drawing(|session, d| session.restyle_selected(d));
    }

    pub fn draw_drag_begin(&mut self, x: f64, y: f64, tolerance: f64) -> bool {
        match self.drawing().cloned() {
            Some(d) => self.draw.drag_begin(&d, (x, y), tolerance),
            None => false,
        }
    }

    pub fn draw_drag_end(&mut self) {
        self.edit_drawing(|session, d| session.drag_end(d));
    }
}
