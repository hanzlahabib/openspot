//! Access-point markers with manual positions (V-12): a hand-placed position overrides the
//! automatic estimate and is stored per snapshot.

use openspot_wlan::MacAddress;

use super::Survey;

/// Marker the user can grab: farther than this (screen px) from its center is a miss.
/// Slightly more than the drawn half-size so a fingertip-sized error still hits.
pub const MARKER_HIT_RADIUS_PX: f32 = 14.0;

/// Where an AP marker is drawn and whether the position came from the user.
#[derive(Debug, Clone, PartialEq)]
pub struct ApMarker {
    pub bssid: MacAddress,
    pub ssid: String,
    pub x: f64,
    pub y: f64,
    pub manual: bool,
}

/// A marker being dragged; the stored position changes only when the drag ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApDrag {
    pub bssid: MacAddress,
    pub x: f64,
    pub y: f64,
}

/// The marker (by BSSID) nearest to `cursor` within `radius`, given screen positions.
pub fn hit_marker(markers: &[(MacAddress, (f32, f32))], cursor: (f32, f32), radius: f32) -> Option<MacAddress> {
    markers
        .iter()
        .map(|(b, p)| (*b, (p.0 - cursor.0).hypot(p.1 - cursor.1)))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(b, _)| b)
}

impl Survey {
    /// Markers for the selected networks: the manual position if set, else the automatic
    /// estimate (needs three readings). A marker being dragged follows the pointer.
    pub fn ap_markers(&self) -> Vec<ApMarker> {
        let canvas = self.project.canvas.snapshot(self.snapshot().id);
        let names: std::collections::BTreeMap<_, _> = self.snapshot().networks().into_iter().map(|(b, s, _)| (b, s)).collect();
        let estimates = self.ap_estimates();
        self.selected
            .iter()
            .filter_map(|b| {
                let ssid = names.get(b).cloned().unwrap_or_default();
                if let Some(d) = self.ap_drag.filter(|d| d.bssid == *b) {
                    return Some(ApMarker { bssid: *b, ssid, x: d.x, y: d.y, manual: true });
                }
                if let Some(&(x, y)) = canvas.and_then(|c| c.manual_aps.get(b)) {
                    return Some(ApMarker { bssid: *b, ssid, x, y, manual: true });
                }
                let est = estimates.iter().find(|e| e.0 == *b)?;
                Some(ApMarker { bssid: *b, ssid, x: est.2.x, y: est.2.y, manual: false })
            })
            .collect()
    }

    pub fn snapshot_has_manual_aps(&self) -> bool {
        self.project.canvas.snapshot(self.snapshot().id).is_some_and(|c| !c.manual_aps.is_empty())
    }

    /// Fixes an AP at map position `(x, y)`, clamped to the plan so it cannot be lost
    /// off-screen. Non-finite positions are ignored.
    pub fn set_manual_ap(&mut self, bssid: MacAddress, x: f64, y: f64) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        let (max_x, max_y) = self.zone().map.as_ref().map_or((f64::MAX, f64::MAX), |m| (f64::from(m.width), f64::from(m.height)));
        let pos = (x.clamp(0.0, max_x), y.clamp(0.0, max_y));
        let id = self.snapshot().id;
        let aps = &mut self.project.canvas.snapshot_mut(id).manual_aps;
        if aps.insert(bssid, pos) != Some(pos) {
            self.touch();
        }
    }

    /// Returns an AP to its automatic estimate.
    pub fn reset_manual_ap(&mut self, bssid: &MacAddress) -> bool {
        let id = self.snapshot().id;
        let removed = self.project.canvas.snapshot_mut(id).manual_aps.remove(bssid).is_some();
        if removed {
            self.touch();
        }
        removed
    }

    pub fn reset_all_manual_aps(&mut self) -> usize {
        let id = self.snapshot().id;
        let aps = &mut self.project.canvas.snapshot_mut(id).manual_aps;
        let n = aps.len();
        aps.clear();
        if n > 0 {
            self.touch();
        }
        n
    }

    /// Starts dragging the marker of `bssid` from its current position.
    pub fn begin_ap_drag(&mut self, bssid: MacAddress) -> bool {
        let Some(m) = self.ap_markers().into_iter().find(|m| m.bssid == bssid) else { return false };
        self.ap_drag = Some(ApDrag { bssid, x: m.x, y: m.y });
        true
    }

    pub fn move_ap_drag(&mut self, x: f64, y: f64) {
        if let Some(d) = &mut self.ap_drag {
            (d.x, d.y) = (x, y);
        }
    }

    /// Ends the drag, storing the final position as the manual one.
    pub fn end_ap_drag(&mut self) {
        if let Some(d) = self.ap_drag.take() {
            self.set_manual_ap(d.bssid, d.x, d.y);
        }
    }

    pub fn cancel_ap_drag(&mut self) {
        self.ap_drag = None;
    }
}
