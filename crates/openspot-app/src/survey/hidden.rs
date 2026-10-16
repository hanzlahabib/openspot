//! Hidden networks in a survey (V-10): per-snapshot include/exclude and labels.

use std::borrow::Cow;

use openspot_model::{HiddenSighting, Snapshot, hidden_networks, without_hidden};
use openspot_wlan::MacAddress;

use super::Survey;

/// One row of the hidden-network list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenRow {
    pub sighting: HiddenSighting,
    pub included: bool,
    pub label: String,
}

impl Survey {
    /// The current snapshot as the heatmap, AP estimates and exports should see it: without
    /// the hidden networks the user excluded. All data consumers go through this.
    pub fn filtered_snapshot(&self) -> Cow<'_, Snapshot> {
        match self.project.canvas.snapshot(self.snapshot().id) {
            Some(c) => without_hidden(self.snapshot(), &c.hidden_excluded),
            None => Cow::Borrowed(self.snapshot()),
        }
    }

    pub fn hidden_rows(&self) -> Vec<HiddenRow> {
        let canvas = self.project.canvas.snapshot(self.snapshot().id);
        hidden_networks(self.snapshot())
            .into_iter()
            .map(|sighting| HiddenRow {
                included: !canvas.is_some_and(|c| c.hidden_excluded.contains(&sighting.bssid)),
                label: canvas.and_then(|c| c.hidden_labels.get(&sighting.bssid).cloned()).unwrap_or_default(),
                sighting,
            })
            .collect()
    }

    pub fn set_hidden_included(&mut self, bssid: MacAddress, included: bool) {
        let id = self.snapshot().id;
        let set = &mut self.project.canvas.snapshot_mut(id).hidden_excluded;
        let excluded_now = set.contains(&bssid);
        if included == excluded_now {
            if included {
                set.remove(&bssid);
            } else {
                set.insert(bssid);
            }
            self.touch();
        }
    }

    /// Sets (or, when blank, clears) the label of a hidden network.
    pub fn set_hidden_label(&mut self, bssid: MacAddress, label: &str) {
        let id = self.snapshot().id;
        let label = label.trim();
        let labels = &mut self.project.canvas.snapshot_mut(id).hidden_labels;
        let changed = if label.is_empty() { labels.remove(&bssid).is_some() } else { labels.insert(bssid, label.to_owned()).as_deref() != Some(label) };
        if changed {
            self.touch();
        }
    }
}
