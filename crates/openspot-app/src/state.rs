//! UI-independent Inspector state: per-AP history, filtering and sorting.

use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};

use openspot_wlan::{AccessPoint, Band, MacAddress};

/// Samples kept per AP. At the fastest 1 s scan interval this is one hour, which is
/// the longest chart window offered; older samples are dropped.
pub const MAX_SAMPLES: usize = 3600;

/// One RSSI observation, `t` in seconds since app start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub t: f64,
    pub rssi_dbm: i32,
}

#[derive(Debug, Clone)]
pub struct Track {
    /// Most recent sighting (freshest IEs and RSSI).
    pub ap: AccessPoint,
    pub samples: VecDeque<Sample>,
    pub first_seen: f64,
    pub last_seen: f64,
}

/// Signal statistics over a track's retained samples, in dBm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RssiStats {
    pub avg: f64,
    pub min: i32,
    pub max: i32,
}

impl Track {
    /// `None` only for an empty track, which ingest never produces.
    pub fn stats(&self) -> Option<RssiStats> {
        let (min, max) = self.samples.iter().fold((i32::MAX, i32::MIN), |(lo, hi), s| (lo.min(s.rssi_dbm), hi.max(s.rssi_dbm)));
        if self.samples.is_empty() {
            return None;
        }
        let sum: i64 = self.samples.iter().map(|s| i64::from(s.rssi_dbm)).sum();
        Some(RssiStats { avg: sum as f64 / self.samples.len() as f64, min, max })
    }
}

#[derive(Debug, Default)]
pub struct Tracker {
    tracks: BTreeMap<MacAddress, Track>,
}

impl Tracker {
    pub fn ingest(&mut self, scan: Vec<AccessPoint>, t: f64) {
        for ap in scan {
            let sample = Sample { t, rssi_dbm: ap.rssi_dbm };
            let track = self.tracks.entry(ap.bssid).or_insert_with(|| Track {
                ap: ap.clone(),
                samples: VecDeque::new(),
                first_seen: t,
                last_seen: t,
            });
            track.samples.push_back(sample);
            if track.samples.len() > MAX_SAMPLES {
                track.samples.pop_front();
            }
            track.last_seen = t;
            track.ap = ap;
        }
    }

    /// Drops APs not seen for `max_age` seconds.
    pub fn prune(&mut self, now: f64, max_age: f64) {
        self.tracks.retain(|_, tr| now - tr.last_seen <= max_age);
    }

    /// Latest sighting of every tracked AP (unfiltered: hidden rows still interfere).
    pub fn access_points(&self) -> Vec<AccessPoint> {
        self.tracks.values().map(|t| t.ap.clone()).collect()
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
    }

    pub fn get(&self, bssid: &MacAddress) -> Option<&Track> {
        self.tracks.get(bssid)
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Tracks passing `filter`, ordered by `sort`.
    pub fn view(&self, filter: &Filter, sort: Sort) -> Vec<&Track> {
        let mut rows: Vec<&Track> = self.tracks.values().filter(|t| filter.matches(&t.ap)).collect();
        rows.sort_by(|a, b| {
            let ord = sort.key.compare(a, b).then_with(|| a.ap.bssid.cmp(&b.ap.bssid));
            if sort.descending { ord.reverse() } else { ord }
        });
        rows
    }
}

#[derive(Debug, Clone)]
pub struct Filter {
    pub text: String,
    pub ghz2_4: bool,
    pub ghz5: bool,
    pub ghz6: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Self { text: String::new(), ghz2_4: true, ghz5: true, ghz6: true }
    }
}

impl Filter {
    pub fn matches(&self, ap: &AccessPoint) -> bool {
        let band_ok = match ap.band {
            Some(Band::Ghz2_4) => self.ghz2_4,
            Some(Band::Ghz5) => self.ghz5,
            Some(Band::Ghz6) => self.ghz6,
            None => true,
        };
        if !band_ok {
            return false;
        }
        let needle = self.text.trim().to_lowercase();
        needle.is_empty()
            || ap.ssid.to_lowercase().contains(&needle)
            || ap.bssid.to_string().to_lowercase().contains(&needle)
            || ap.bssid.vendor().is_some_and(|v| v.to_lowercase().contains(&needle))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Ssid,
    Bssid,
    Rssi,
    Channel,
    Band,
    Width,
    Vendor,
    AvgRssi,
    MinRssi,
    MaxRssi,
    NetworkType,
    FirstSeen,
    LastSeen,
}

impl SortKey {
    /// Numeric "more is better" columns start with the best first; text columns A to Z.
    fn defaults_to_descending(self) -> bool {
        matches!(self, Self::Rssi | Self::AvgRssi | Self::MinRssi | Self::MaxRssi | Self::LastSeen | Self::FirstSeen)
    }

    fn compare(self, a: &Track, b: &Track) -> Ordering {
        let (x, y) = (&a.ap, &b.ap);
        let stat = |t: &Track| t.stats().unwrap_or(RssiStats { avg: f64::MIN, min: i32::MIN, max: i32::MIN });
        match self {
            Self::Ssid => x.ssid.to_lowercase().cmp(&y.ssid.to_lowercase()),
            Self::Bssid => x.bssid.cmp(&y.bssid),
            Self::Rssi => x.rssi_dbm.cmp(&y.rssi_dbm),
            Self::Channel => x.channel.cmp(&y.channel),
            Self::Band => x.band.cmp(&y.band),
            Self::Width => x.width.cmp(&y.width),
            Self::Vendor => x.bssid.vendor().cmp(&y.bssid.vendor()),
            Self::AvgRssi => stat(a).avg.total_cmp(&stat(b).avg),
            Self::MinRssi => stat(a).min.cmp(&stat(b).min),
            Self::MaxRssi => stat(a).max.cmp(&stat(b).max),
            Self::NetworkType => x.network_type.cmp(&y.network_type),
            // More recent sightings are "greater", so descending puts the freshest first.
            Self::LastSeen => a.last_seen.total_cmp(&b.last_seen),
            Self::FirstSeen => a.first_seen.total_cmp(&b.first_seen),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for Sort {
    /// Strongest signal first, like every Wi-Fi scanner.
    fn default() -> Self {
        Self { key: SortKey::Rssi, descending: true }
    }
}

impl Sort {
    /// Clicking the active column flips direction; a new column starts ascending, except
    /// signal-like columns, where strongest/freshest first is the useful default.
    pub fn toggle(&mut self, key: SortKey) {
        if self.key == key {
            self.descending = !self.descending;
        } else {
            *self = Self { key, descending: key.defaults_to_descending() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(last: u8, ssid: &str, rssi: i32, khz: u32) -> AccessPoint {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, khz, true, 100, vec![])
    }

    #[test]
    fn ingest_accumulates_history_and_updates_latest() {
        let mut t = Tracker::default();
        t.ingest(vec![ap(1, "a", -50, 2_437_000)], 0.0);
        t.ingest(vec![ap(1, "a2", -60, 2_437_000)], 2.0);
        let tr = t.get(&MacAddress([0, 0, 0, 0, 0, 1])).unwrap();
        assert_eq!(tr.samples.len(), 2);
        assert_eq!(tr.ap.ssid, "a2");
        assert_eq!(tr.last_seen, 2.0);
    }

    #[test]
    fn history_is_capped() {
        let mut t = Tracker::default();
        for i in 0..MAX_SAMPLES + 10 {
            t.ingest(vec![ap(1, "a", -50, 2_437_000)], i as f64);
        }
        let tr = t.get(&MacAddress([0, 0, 0, 0, 0, 1])).unwrap();
        assert_eq!(tr.samples.len(), MAX_SAMPLES);
        assert_eq!(tr.samples.front().unwrap().t, 10.0);
    }

    #[test]
    fn prune_removes_stale_only() {
        let mut t = Tracker::default();
        t.ingest(vec![ap(1, "old", -50, 2_437_000)], 0.0);
        t.ingest(vec![ap(2, "new", -50, 2_437_000)], 100.0);
        t.prune(110.0, 30.0);
        assert_eq!(t.len(), 1);
        assert!(t.get(&MacAddress([0, 0, 0, 0, 0, 2])).is_some());
    }

    #[test]
    fn filter_by_band_and_text() {
        let mut t = Tracker::default();
        t.ingest(vec![ap(1, "Home", -50, 2_437_000), ap(2, "Office", -60, 5_180_000)], 0.0);
        let mut f = Filter::default();
        assert_eq!(t.view(&f, Sort::default()).len(), 2);
        f.ghz2_4 = false;
        assert_eq!(t.view(&f, Sort::default())[0].ap.ssid, "Office");
        f.ghz2_4 = true;
        f.text = "  hOmE ".into();
        assert_eq!(t.view(&f, Sort::default()).len(), 1);
        f.text = "00:00:00:00:00:02".into();
        assert_eq!(t.view(&f, Sort::default())[0].ap.ssid, "Office");
    }

    #[test]
    fn filter_matches_vendor_names() {
        let mut t = Tracker::default();
        let vm = AccessPoint::from_beacon("vm".into(), MacAddress([0x00, 0x50, 0x56, 0, 0, 1]), -50, 2_437_000, true, 100, vec![]);
        t.ingest(vec![vm, ap(2, "other", -60, 2_437_000)], 0.0);
        let f = Filter { text: "vmware".into(), ..Filter::default() };
        let rows = t.view(&f, Sort::default());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ap.ssid, "vm");
    }

    #[test]
    fn stats_cover_the_whole_history() {
        let mut t = Tracker::default();
        for (i, r) in [-50, -70, -60].into_iter().enumerate() {
            t.ingest(vec![ap(1, "a", r, 2_437_000)], i as f64);
        }
        let tr = t.get(&MacAddress([0, 0, 0, 0, 0, 1])).unwrap();
        let st = tr.stats().unwrap();
        assert_eq!((st.min, st.max), (-70, -50));
        assert!((st.avg + 60.0).abs() < 1e-9);
        assert_eq!(tr.first_seen, 0.0);
        assert_eq!(tr.last_seen, 2.0);
    }

    #[test]
    fn sorting_by_stats_and_recency() {
        let mut t = Tracker::default();
        // AP 1: steady -60. AP 2: swings -40..-90 (higher max, lower min, lower avg... same avg -65 vs -60).
        for (i, (r1, r2)) in [(-60, -40), (-60, -90)].into_iter().enumerate() {
            t.ingest(vec![ap(1, "steady", r1, 2_437_000), ap(2, "swing", r2, 2_437_000)], i as f64);
        }
        t.ingest(vec![ap(1, "steady", -60, 2_437_000)], 5.0); // AP 2 not seen again
        let first = |key| {
            let s = Sort { key, descending: true };
            t.view(&Filter::default(), s)[0].ap.ssid.clone()
        };
        assert_eq!(first(SortKey::MaxRssi), "swing");
        assert_eq!(first(SortKey::MinRssi), "steady");
        assert_eq!(first(SortKey::AvgRssi), "steady");
        assert_eq!(first(SortKey::LastSeen), "steady");
        let mut s = Sort::default();
        s.toggle(SortKey::AvgRssi);
        assert!(s.descending);
        s.toggle(SortKey::Ssid);
        assert!(!s.descending);
    }

    #[test]
    fn sort_toggle_and_order() {
        let mut t = Tracker::default();
        t.ingest(vec![ap(1, "b", -70, 2_437_000), ap(2, "A", -40, 2_437_000)], 0.0);
        let mut s = Sort::default();
        assert_eq!(t.view(&Filter::default(), s)[0].ap.ssid, "A"); // strongest first
        s.toggle(SortKey::Ssid);
        assert_eq!(s, Sort { key: SortKey::Ssid, descending: false });
        assert_eq!(t.view(&Filter::default(), s)[0].ap.ssid, "A"); // case-insensitive
        s.toggle(SortKey::Ssid);
        assert_eq!(t.view(&Filter::default(), s)[0].ap.ssid, "b");
    }

    #[test]
    fn sorting_by_first_seen_and_network_type() {
        use openspot_wlan::NetworkType;
        let mut t = Tracker::default();
        t.ingest(vec![ap(1, "old", -50, 2_437_000)], 0.0);
        let adhoc = ap(2, "adhoc", -50, 2_437_000).with_network_type(NetworkType::AdHoc);
        t.ingest(vec![ap(1, "old", -50, 2_437_000), adhoc], 10.0);
        let first = |key, descending| {
            let s = Sort { key, descending };
            t.view(&Filter::default(), s)[0].ap.ssid.clone()
        };
        assert_eq!(first(SortKey::FirstSeen, true), "adhoc", "newest first when descending");
        assert_eq!(first(SortKey::FirstSeen, false), "old");
        assert_eq!(first(SortKey::NetworkType, false), "old", "infrastructure sorts before ad-hoc");
        assert_eq!(first(SortKey::NetworkType, true), "adhoc");
    }
}
