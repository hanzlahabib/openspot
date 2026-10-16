//! Per-AP coverage mode: show one network's signal map, with quick switching.

use openspot_wlan::MacAddress;

use super::Survey;

/// Next AP when stepping through `list` by `delta`. The cycle runs "off" (all networks)
/// -> first -> ... -> last -> "off", so one button pair reaches every map and the
/// composite. A current AP missing from `list` behaves like "off".
pub fn step(list: &[MacAddress], current: Option<MacAddress>, delta: isize) -> Option<MacAddress> {
    let len = list.len() as isize;
    if len == 0 {
        return None;
    }
    // Slot 0 is "off"; slots 1..=len are the APs.
    let slot = current.and_then(|c| list.iter().position(|b| *b == c)).map_or(0, |i| i as isize + 1);
    let next = (slot + delta).rem_euclid(len + 1);
    (next > 0).then(|| list[next as usize - 1])
}

impl Survey {
    /// APs the quick switch cycles through: the ticked networks, or all heard ones if none are ticked.
    pub fn per_ap_candidates(&self) -> Vec<MacAddress> {
        let nets = self.snapshot().networks();
        if self.selected.is_empty() {
            nets.into_iter().map(|n| n.0).collect()
        } else {
            nets.into_iter().map(|n| n.0).filter(|b| self.selected.contains(b)).collect()
        }
    }

    pub fn step_per_ap(&mut self, delta: isize) {
        self.per_ap = step(&self.per_ap_candidates(), self.per_ap, delta);
        self.generation += 1;
    }

    pub fn set_per_ap(&mut self, bssid: Option<MacAddress>) {
        if self.per_ap != bssid {
            self.per_ap = bssid;
            self.generation += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survey::MetricChoice;
    use openspot_heatmap::Metric;
    use openspot_wlan::AccessPoint;

    fn mac(last: u8) -> MacAddress {
        MacAddress([0, 0, 0, 0, 0, last])
    }

    #[test]
    fn stepping_cycles_through_aps_and_off() {
        let list = [mac(1), mac(2), mac(3)];
        assert_eq!(step(&list, None, 1), Some(mac(1)));
        assert_eq!(step(&list, Some(mac(1)), 1), Some(mac(2)));
        assert_eq!(step(&list, Some(mac(3)), 1), None, "past the last returns to all networks");
        assert_eq!(step(&list, None, -1), Some(mac(3)));
        assert_eq!(step(&list, Some(mac(1)), -1), None);
        assert_eq!(step(&list, Some(mac(9)), 1), Some(mac(1)), "unknown current acts like off");
        assert_eq!(step(&[], Some(mac(1)), 1), None);
    }

    fn survey_with_two_aps() -> Survey {
        let mut s = Survey::default();
        let ap = |last: u8, rssi| AccessPoint::from_beacon("n".into(), mac(last), rssi, 2_437_000, true, 100, vec![]);
        s.add_scan(1.0, 1.0, vec![ap(1, -40), ap(2, -80)]);
        s
    }

    #[test]
    fn per_ap_overrides_the_metric_with_that_aps_signal() {
        let mut s = survey_with_two_aps();
        s.metric_choice = MetricChoice::ApCount;
        s.set_per_ap(Some(mac(2)));
        assert_eq!(s.metric(), Metric::Signal(vec![mac(2)]));
        s.set_per_ap(None);
        assert_eq!(s.metric(), Metric::ApCount { min_rssi_dbm: -80 });
    }

    #[test]
    fn candidates_prefer_ticked_networks_and_selection_changes_clear_the_mode() {
        let mut s = survey_with_two_aps();
        assert_eq!(s.per_ap_candidates(), vec![mac(1), mac(2)]);
        s.selected.insert(mac(2));
        assert_eq!(s.per_ap_candidates(), vec![mac(2)]);
        s.step_per_ap(1);
        assert_eq!(s.per_ap, Some(mac(2)));
        // Switching snapshot drops the mode: the AP may not exist there.
        s.add_snapshot();
        assert_eq!(s.per_ap, None);
    }

    #[test]
    fn changing_the_ap_invalidates_the_heatmap_cache() {
        let mut s = survey_with_two_aps();
        let key = s.heat_key();
        s.set_per_ap(Some(mac(1)));
        assert_ne!(s.heat_key(), key);
    }
}
