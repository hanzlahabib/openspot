//! Inspector event log: what happened and when. Pure data and rules; the UI lives in
//! `ui/event_log.rs`.

use std::collections::{BTreeMap, VecDeque};

use openspot_wlan::{AccessPoint, MacAddress};

use crate::csvio;

/// Hard cap on stored events. A scan event is added every scan, so at the fastest 1 s interval
/// this is about 83 minutes of history, a few hundred KB; the oldest events go first. The
/// time-based erase setting usually trims earlier than this.
pub const MAX_EVENTS: usize = 5000;

/// Network names kept per scan event; the counts always cover every network.
pub const MAX_LISTED_APS: usize = 5;

/// Presets for "erase log entries older than", in seconds; 0 means never.
pub const ERASE_PRESETS_SECS: [u64; 4] = [0, 15 * 60, 60 * 60, 24 * 60 * 60];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Scan { total: usize, added: usize, removed: usize, added_names: Vec<String>, removed_names: Vec<String> },
    Paused,
    Resumed,
    AdapterChanged { name: String },
    IntervalChanged { secs: u64 },
    Error { message: String, repeats: u32 },
}

/// Categories the log filter toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Scan,
    PauseResume,
    Settings,
    Error,
}

impl Category {
    pub const ALL: [Self; 4] = [Self::Scan, Self::PauseResume, Self::Settings, Self::Error];

    pub fn label(self) -> &'static str {
        match self {
            Self::Scan => "Scans",
            Self::PauseResume => "Pause/resume",
            Self::Settings => "Adapter & interval",
            Self::Error => "Errors",
        }
    }
}

impl EventKind {
    pub fn category(&self) -> Category {
        match self {
            Self::Scan { .. } => Category::Scan,
            Self::Paused | Self::Resumed => Category::PauseResume,
            Self::AdapterChanged { .. } | Self::IntervalChanged { .. } => Category::Settings,
            Self::Error { .. } => Category::Error,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Scan { total, added, removed, added_names, removed_names } => {
                let mut text = format!("Scan: {total} networks");
                if *added == 0 && *removed == 0 {
                    text.push_str(", no changes");
                }
                if *added > 0 {
                    text.push_str(&format!(", {added} new ({})", list(added_names, *added)));
                }
                if *removed > 0 {
                    text.push_str(&format!(", {removed} gone ({})", list(removed_names, *removed)));
                }
                text
            }
            Self::Paused => "Scanning paused".to_owned(),
            Self::Resumed => "Scanning resumed".to_owned(),
            Self::AdapterChanged { name } => format!("Adapter changed to {name}"),
            Self::IntervalChanged { secs } => format!("Scan interval changed to {secs} s"),
            Self::Error { message, repeats } if *repeats > 1 => format!("Scan error: {message} (x{repeats})"),
            Self::Error { message, .. } => format!("Scan error: {message}"),
        }
    }
}

fn list(names: &[String], total: usize) -> String {
    let mut text = names.join(", ");
    if total > names.len() {
        text.push_str(&format!(", +{} more", total - names.len()));
    }
    text
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Seconds since app start; the clock retention is measured on.
    pub t: f64,
    /// Wall-clock time for display and export.
    pub unix_secs: i64,
    pub kind: EventKind,
}

/// Which events the viewer shows (and export writes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventFilter {
    pub text: String,
    hidden: Vec<Category>,
}

impl EventFilter {
    pub fn shows(&self, c: Category) -> bool {
        !self.hidden.contains(&c)
    }

    pub fn set_shown(&mut self, c: Category, show: bool) {
        self.hidden.retain(|h| *h != c);
        if !show {
            self.hidden.push(c);
        }
    }

    pub fn matches(&self, e: &Event) -> bool {
        if !self.shows(e.kind.category()) {
            return false;
        }
        let needle = self.text.trim().to_lowercase();
        needle.is_empty() || e.kind.message().to_lowercase().contains(&needle)
    }
}

/// BSSID to display label ("SSID (BSSID)") of the networks in the previous scan.
type Seen = BTreeMap<MacAddress, String>;

/// How a scan differs from the one before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanDiff {
    /// Networks not in the previous scan.
    pub new: Vec<MacAddress>,
    /// True when there was no previous scan to compare with (first scan, or after the adapter
    /// changed), so "new" just means "everything".
    pub baseline: bool,
}

impl ScanDiff {
    /// Whether something genuinely new showed up, as opposed to the initial inventory.
    pub fn has_new_networks(&self) -> bool {
        !self.baseline && !self.new.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct EventLog {
    events: VecDeque<Event>,
    previous_scan: Seen,
    has_baseline: bool,
}

fn label_for(ap: &AccessPoint) -> String {
    if ap.ssid.is_empty() { format!("<hidden> {}", ap.bssid) } else { format!("{} {}", ap.ssid, ap.bssid) }
}

impl EventLog {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Oldest first.
    #[cfg(test)]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Event> {
        self.events.iter()
    }

    pub fn record(&mut self, kind: EventKind, t: f64, unix_secs: i64) {
        if let EventKind::Error { message, .. } = &kind
            && let Some(Event { kind: EventKind::Error { message: last, repeats }, .. }) = self.events.back_mut()
            && last == message
        {
            // A persistent failure would otherwise add a line every scan interval.
            *repeats += 1;
            return;
        }
        self.events.push_back(Event { t, unix_secs, kind });
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }

    /// Records a scan summary against the previous scan.
    pub fn record_scan(&mut self, aps: &[AccessPoint], t: f64, unix_secs: i64) -> ScanDiff {
        let current: Seen = aps.iter().map(|ap| (ap.bssid, label_for(ap))).collect();
        let new_macs: Vec<MacAddress> = current.keys().filter(|m| !self.previous_scan.contains_key(m)).copied().collect();
        let gone: Vec<&String> = self.previous_scan.iter().filter(|(m, _)| !current.contains_key(m)).map(|(_, l)| l).collect();
        let names = |macs: &[MacAddress], from: &Seen| macs.iter().take(MAX_LISTED_APS).map(|m| from[m].clone()).collect();
        let kind = EventKind::Scan {
            total: current.len(),
            added: new_macs.len(),
            removed: gone.len(),
            added_names: names(&new_macs, &current),
            removed_names: gone.iter().take(MAX_LISTED_APS).map(|l| (*l).clone()).collect(),
        };
        let baseline = !self.has_baseline;
        self.previous_scan = current;
        self.has_baseline = true;
        self.record(kind, t, unix_secs);
        ScanDiff { new: new_macs, baseline }
    }

    /// Forgets the previous scan (after an adapter change its results are not comparable).
    pub fn reset_scan_baseline(&mut self) {
        self.previous_scan.clear();
        self.has_baseline = false;
    }

    /// Drops events older than `max_age_secs` (0 = never).
    pub fn erase_older_than(&mut self, now: f64, max_age_secs: u64) {
        if max_age_secs == 0 {
            return;
        }
        let max_age = max_age_secs as f64;
        self.events.retain(|e| now - e.t <= max_age);
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn filtered(&self, filter: &EventFilter) -> Vec<&Event> {
        self.events.iter().filter(|e| filter.matches(e)).collect()
    }

    /// Plain-text export, oldest first, one event per line.
    pub fn to_text(&self, filter: &EventFilter) -> String {
        self.filtered(filter).iter().map(|e| format!("{}  {}\n", format_utc(e.unix_secs), e.kind.message())).collect()
    }

    /// CSV export (`time_utc,category,message`), oldest first. Messages contain network names
    /// chosen by strangers, so every cell goes through the formula guard.
    pub fn to_csv(&self, filter: &EventFilter) -> String {
        let mut out = csvio::write_record(&["time_utc", "category", "message"], ',');
        for e in self.filtered(filter) {
            let cells = [format_utc(e.unix_secs), e.kind.category().label().to_owned(), e.kind.message()];
            let guarded: Vec<_> = cells.iter().map(|c| csvio::neutralize_formula(c)).collect();
            out.push_str(&csvio::write_record(&guarded, ','));
        }
        out
    }
}

/// `YYYY-MM-DD HH:MM:SS` in UTC. Uses the days-from-civil inverse by Howard Hinnant
/// (public-domain algorithm) so no date library is needed.
pub fn format_utc(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(last: u8, ssid: &str) -> AccessPoint {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0, 0, 0, 0, 0, last]), -50, 2_437_000, true, 100, vec![])
    }

    fn scan_counts(e: &Event) -> (usize, usize, usize) {
        match &e.kind {
            EventKind::Scan { total, added, removed, .. } => (*total, *added, *removed),
            other => panic!("not a scan: {other:?}"),
        }
    }

    #[test]
    fn scan_events_summarise_changes_against_the_previous_scan() {
        let mut log = EventLog::default();
        let first = log.record_scan(&[ap(1, "a"), ap(2, "b")], 0.0, 0);
        assert_eq!(first.new.len(), 2, "everything is new on the first scan");
        assert!(first.baseline && !first.has_new_networks(), "but that is the initial inventory, not news");
        let second = log.record_scan(&[ap(2, "b"), ap(3, "c")], 2.0, 2);
        assert_eq!(second.new, vec![MacAddress([0, 0, 0, 0, 0, 3])]);
        assert!(second.has_new_networks());
        let third = log.record_scan(&[ap(2, "b"), ap(3, "c")], 4.0, 4);
        assert!(third.new.is_empty() && !third.has_new_networks());
        let counts: Vec<_> = log.iter().map(scan_counts).collect();
        assert_eq!(counts, vec![(2, 2, 0), (2, 1, 1), (2, 0, 0)]);
        let second = log.iter().nth(1).unwrap().kind.message();
        assert!(second.contains("1 new (c ") && second.contains("1 gone (a "), "{second}");
        assert!(log.iter().nth(2).unwrap().kind.message().contains("no changes"));
    }

    #[test]
    fn listed_names_are_capped_but_counts_are_not() {
        let mut log = EventLog::default();
        let aps: Vec<_> = (1..=20).map(|i| ap(i, "n")).collect();
        log.record_scan(&aps, 0.0, 0);
        let EventKind::Scan { added, added_names, .. } = &log.iter().next().unwrap().kind else { panic!() };
        assert_eq!((*added, added_names.len()), (20, MAX_LISTED_APS));
        assert!(log.iter().next().unwrap().kind.message().contains("+15 more"));
    }

    #[test]
    fn baseline_reset_makes_the_next_scan_all_new() {
        let mut log = EventLog::default();
        log.record_scan(&[ap(1, "a")], 0.0, 0);
        log.reset_scan_baseline();
        let again = log.record_scan(&[ap(1, "a")], 1.0, 1);
        assert_eq!(again.new.len(), 1);
        assert!(again.baseline, "after a reset the next scan is a fresh inventory");
        assert!(!log.record_scan(&[ap(1, "a")], 2.0, 2).baseline);
    }

    #[test]
    fn repeated_identical_errors_collapse_into_one_line() {
        let mut log = EventLog::default();
        let err = |m: &str| EventKind::Error { message: m.into(), repeats: 1 };
        log.record(err("radio off"), 0.0, 0);
        log.record(err("radio off"), 2.0, 2);
        log.record(err("radio off"), 4.0, 4);
        assert_eq!(log.len(), 1);
        assert!(log.iter().next().unwrap().kind.message().ends_with("(x3)"));
        log.record(err("other"), 6.0, 6);
        log.record(err("radio off"), 8.0, 8);
        assert_eq!(log.len(), 3, "a different message in between starts a new line");
    }

    #[test]
    fn storage_is_bounded_and_drops_the_oldest() {
        let mut log = EventLog::default();
        for i in 0..MAX_EVENTS + 25 {
            log.record(EventKind::Paused, i as f64, i as i64);
        }
        assert_eq!(log.len(), MAX_EVENTS);
        assert_eq!(log.iter().next().unwrap().t, 25.0);
    }

    #[test]
    fn erase_older_than_uses_the_age_and_zero_means_never() {
        let mut log = EventLog::default();
        for t in [0.0, 100.0, 200.0] {
            log.record(EventKind::Resumed, t, 0);
        }
        log.erase_older_than(250.0, 0);
        assert_eq!(log.len(), 3, "never");
        log.erase_older_than(250.0, 100);
        assert_eq!(log.iter().map(|e| e.t).collect::<Vec<_>>(), vec![200.0], "age 150 and 250 are over the limit");
        log.erase_older_than(300.0, 100);
        assert_eq!(log.len(), 1, "exactly at the limit is kept");
        log.erase_older_than(301.0, 100);
        assert!(log.is_empty());
    }

    #[test]
    fn filter_by_category_and_text() {
        let mut log = EventLog::default();
        log.record(EventKind::Paused, 0.0, 0);
        log.record(EventKind::AdapterChanged { name: "Intel AX210".into() }, 1.0, 1);
        log.record(EventKind::Error { message: "radio off".into(), repeats: 1 }, 2.0, 2);
        let mut f = EventFilter::default();
        assert_eq!(log.filtered(&f).len(), 3);
        f.set_shown(Category::Error, false);
        assert_eq!(log.filtered(&f).len(), 2);
        f.set_shown(Category::Error, true);
        f.text = " AX210 ".into();
        assert_eq!(log.filtered(&f).len(), 1);
        f.text = "nothing".into();
        assert!(log.filtered(&f).is_empty());
    }

    #[test]
    fn utc_formatting_matches_known_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00", "leap day");
        assert_eq!(format_utc(1_709_210_096), "2024-02-29 12:34:56");
        assert_eq!(format_utc(-1), "1969-12-31 23:59:59");
        assert_eq!(format_utc(4_102_444_799), "2099-12-31 23:59:59");
    }

    #[test]
    fn text_export_respects_the_filter_and_order() {
        let mut log = EventLog::default();
        log.record(EventKind::Paused, 0.0, 0);
        log.record(EventKind::Resumed, 1.0, 60);
        let mut f = EventFilter::default();
        assert_eq!(log.to_text(&f), "1970-01-01 00:00:00  Scanning paused\n1970-01-01 00:01:00  Scanning resumed\n");
        f.text = "resumed".into();
        assert_eq!(log.to_text(&f), "1970-01-01 00:01:00  Scanning resumed\n");
    }

    #[test]
    fn csv_export_escapes_hostile_network_names() {
        let mut log = EventLog::default();
        // An SSID is chosen by whoever runs the access point: it can hold commas, quotes and a formula.
        let evil = AccessPoint::from_beacon("=HYPERLINK(\"http://x\",\"a\")".into(), MacAddress([0, 0, 0, 0, 0, 1]), -50, 2_437_000, true, 100, vec![]);
        log.record_scan(&[evil], 0.0, 0);
        log.record(EventKind::Error { message: "-cmd".into(), repeats: 1 }, 1.0, 1);
        let csv = log.to_csv(&EventFilter::default());
        let parsed = csvio::parse(&csv, ',', csvio::ParseMode::Strict).unwrap();
        assert_eq!(parsed[0].fields, vec!["time_utc", "category", "message"]);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[1].fields.len(), 3, "commas inside the SSID did not split the row");
        assert!(parsed[1].fields[2].contains("=HYPERLINK(\"http://x\",\"a\")"), "{:?}", parsed[1].fields[2]);
        assert_eq!(parsed[2].fields[2], "Scan error: -cmd");
        // No cell may be read as a formula, whatever the event text contains.
        assert!(parsed.iter().flat_map(|r| &r.fields).all(|c| !c.starts_with(['=', '+', '@'])));
    }
}
