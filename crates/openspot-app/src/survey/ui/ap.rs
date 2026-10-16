//! Survey UI: dragging AP markers.

use eframe::egui::{Key, PointerButton, Pos2, Response};

use crate::survey::Survey;
use crate::survey::ap_manual::{MARKER_HIT_RADIUS_PX, hit_marker};

/// Handles grabbing, moving and dropping an AP marker. Returns whether a marker drag is in
/// progress, so the canvas does not pan at the same time.
pub(super) fn drag(s: &mut Survey, response: &Response, origin: Pos2) -> bool {
    if response.drag_started_by(PointerButton::Primary) && !s.calibration.is_active() {
        if let Some(pos) = response.interact_pointer_pos() {
            let screen: Vec<_> = s
                .ap_markers()
                .iter()
                .map(|m| {
                    let p = s.view.to_screen(origin, (m.x, m.y));
                    (m.bssid, (p.x, p.y))
                })
                .collect();
            if let Some(bssid) = hit_marker(&screen, (pos.x, pos.y), MARKER_HIT_RADIUS_PX) {
                s.begin_ap_drag(bssid);
            }
        }
    }
    let active = s.ap_drag.is_some();
    if active {
        if let Some(pos) = response.interact_pointer_pos() {
            let (x, y) = s.view.to_map(origin, pos);
            s.move_ap_drag(x, y);
        }
        if response.ctx.input(|i| i.key_pressed(Key::Escape)) {
            s.cancel_ap_drag();
        } else if response.drag_stopped() {
            s.end_ap_drag();
        }
    }
    active
}
