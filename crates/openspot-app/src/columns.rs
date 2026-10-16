//! Inspector table columns: identity, visibility, cell values and the signal-percentage mapping.

use std::collections::BTreeSet;

use crate::aliases::Aliases;
use crate::state::{SortKey, Track};

/// Lower end of the signal-percentage scale: at or below this RSSI the signal is 0 %.
pub const PERCENT_FLOOR_DBM: i32 = -100;
/// Upper end of the scale: at or above this RSSI the signal is 100 %.
pub const PERCENT_CEIL_DBM: i32 = -50;

/// Signal quality in percent from RSSI in dBm.
///
/// Convention: the one Microsoft documents for `WLAN_AVAILABLE_NETWORK.wlanSignalQuality`
/// (0 = -100 dBm, 100 = -50 dBm, linear in between), so the figure agrees with what the OS
/// shows for the same network.
pub fn rssi_to_percent(dbm: i32) -> u8 {
    let span = PERCENT_CEIL_DBM - PERCENT_FLOOR_DBM;
    let clamped = dbm.clamp(PERCENT_FLOOR_DBM, PERCENT_CEIL_DBM);
    // The clamp bounds the value to 0..=span, so the cast cannot truncate.
    (f64::from((clamped - PERCENT_FLOOR_DBM) * 100) / f64::from(span)).round() as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Column {
    Ssid,
    Alias,
    Bssid,
    Rssi,
    Percent,
    Avg,
    Min,
    Max,
    Channel,
    Band,
    Width,
    Phy,
    Security,
    Type,
    FirstSeen,
    Seen,
    Vendor,
}

impl Column {
    /// Display order of the table.
    pub const ALL: [Self; 17] = [
        Self::Ssid,
        Self::Alias,
        Self::Bssid,
        Self::Rssi,
        Self::Percent,
        Self::Avg,
        Self::Min,
        Self::Max,
        Self::Channel,
        Self::Band,
        Self::Width,
        Self::Phy,
        Self::Security,
        Self::Type,
        Self::FirstSeen,
        Self::Seen,
        Self::Vendor,
    ];

    /// Stable identifier used in preferences; never change an existing one.
    pub fn key(self) -> &'static str {
        match self {
            Self::Ssid => "ssid",
            Self::Alias => "alias",
            Self::Bssid => "bssid",
            Self::Rssi => "rssi",
            Self::Percent => "percent",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
            Self::Channel => "channel",
            Self::Band => "band",
            Self::Width => "width",
            Self::Phy => "phy",
            Self::Security => "security",
            Self::Type => "type",
            Self::FirstSeen => "first_seen",
            Self::Seen => "seen",
            Self::Vendor => "vendor",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Ssid => "SSID",
            Self::Alias => "Alias",
            Self::Bssid => "BSSID",
            Self::Rssi => "dBm",
            Self::Percent => "%",
            Self::Avg => "Avg",
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Channel => "Ch",
            Self::Band => "Band",
            Self::Width => "Width",
            Self::Phy => "PHY",
            Self::Security => "Security",
            Self::Type => "Type",
            Self::FirstSeen => "First seen",
            Self::Seen => "Seen",
            Self::Vendor => "Vendor",
        }
    }

    /// Wider description for the column picker.
    pub fn description(self) -> &'static str {
        match self {
            Self::Ssid => "Network name (or alias when set)",
            Self::Alias => "Your alias only",
            Self::Rssi => "Signal now, dBm",
            Self::Percent => "Signal now, percent",
            Self::Avg => "Average signal, dBm",
            Self::Min => "Weakest signal, dBm",
            Self::Max => "Strongest signal, dBm",
            Self::Channel => "Primary channel",
            Self::Type => "Infrastructure or ad-hoc",
            Self::FirstSeen => "Time since first sighting",
            Self::Seen => "Time since last sighting",
            _ => self.title(),
        }
    }

    /// Column the table can be sorted by, if any. The percent column orders like dBm.
    pub fn sort_key(self) -> Option<SortKey> {
        match self {
            Self::Ssid => Some(SortKey::Ssid),
            Self::Bssid => Some(SortKey::Bssid),
            Self::Rssi | Self::Percent => Some(SortKey::Rssi),
            Self::Avg => Some(SortKey::AvgRssi),
            Self::Min => Some(SortKey::MinRssi),
            Self::Max => Some(SortKey::MaxRssi),
            Self::Channel => Some(SortKey::Channel),
            Self::Band => Some(SortKey::Band),
            Self::Width => Some(SortKey::Width),
            Self::Type => Some(SortKey::NetworkType),
            Self::FirstSeen => Some(SortKey::FirstSeen),
            Self::Seen => Some(SortKey::LastSeen),
            Self::Vendor => Some(SortKey::Vendor),
            Self::Alias | Self::Phy | Self::Security => None,
        }
    }

    /// Columns shown until the user changes the selection.
    pub fn default_visible(self) -> bool {
        !matches!(self, Self::Alias | Self::Type | Self::FirstSeen)
    }

    /// Initial pixel width in the table.
    pub fn width(self) -> f32 {
        match self {
            Self::Ssid => 190.0,
            Self::Alias | Self::Vendor => 140.0,
            Self::Bssid => 125.0,
            Self::Security => 150.0,
            Self::Phy | Self::Type => 90.0,
            Self::Band | Self::Width | Self::Seen | Self::FirstSeen => 64.0,
            Self::Rssi | Self::Percent | Self::Avg | Self::Min | Self::Max => 50.0,
            Self::Channel => 36.0,
        }
    }
}

/// Which columns are shown. Never empty: the last visible column cannot be hidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSet {
    visible: BTreeSet<Column>,
}

impl Default for ColumnSet {
    fn default() -> Self {
        Self { visible: Column::ALL.into_iter().filter(|c| c.default_visible()).collect() }
    }
}

impl ColumnSet {
    /// Restores from persisted keys. `None` or no recognised key gives the defaults, so a
    /// preferences file from another version can never produce an empty table.
    pub fn from_keys(keys: Option<&[String]>) -> Self {
        let visible: BTreeSet<Column> = keys.into_iter().flatten().filter_map(|k| Column::from_key(k)).collect();
        if visible.is_empty() { Self::default() } else { Self { visible } }
    }

    /// Keys to persist, in display order.
    pub fn to_keys(&self) -> Vec<String> {
        self.iter().map(|c| c.key().to_owned()).collect()
    }

    pub fn contains(&self, c: Column) -> bool {
        self.visible.contains(&c)
    }

    /// Visible columns in display order.
    pub fn iter(&self) -> impl Iterator<Item = Column> + '_ {
        Column::ALL.into_iter().filter(|c| self.visible.contains(c))
    }

    /// Shows or hides `c`. Returns false (and changes nothing) when it would hide the last column.
    pub fn set(&mut self, c: Column, show: bool) -> bool {
        if show {
            self.visible.insert(c);
            true
        } else if self.visible.len() > 1 {
            self.visible.remove(&c);
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Plain-text cell value shared by the table and CSV export, so both always agree.
pub fn raw_value(c: Column, track: &Track, aliases: &Aliases, now: f64) -> String {
    let ap = &track.ap;
    let stats = track.stats();
    match c {
        Column::Ssid => aliases.name_for(&ap.bssid, &ap.ssid),
        Column::Alias => aliases.get(&ap.bssid).unwrap_or_default().to_owned(),
        Column::Bssid => ap.bssid.to_string(),
        Column::Rssi => ap.rssi_dbm.to_string(),
        Column::Percent => rssi_to_percent(ap.rssi_dbm).to_string(),
        Column::Avg => stats.map_or_else(String::new, |s| format!("{:.0}", s.avg)),
        Column::Min => stats.map_or_else(String::new, |s| s.min.to_string()),
        Column::Max => stats.map_or_else(String::new, |s| s.max.to_string()),
        Column::Channel => ap.channel.to_string(),
        Column::Band => ap.band.map_or("?", |b| b.label()).to_owned(),
        Column::Width => ap.width.map_or_else(|| "-".to_owned(), |w| format!("{} MHz", w.mhz())),
        Column::Phy => ap.phy.iter().map(|p| p.label()).collect::<Vec<_>>().join("/"),
        Column::Security => ap.security.iter().map(|s| s.label()).collect::<Vec<_>>().join(" + "),
        Column::Type => ap.network_type.label().to_owned(),
        Column::FirstSeen => age_label(now - track.first_seen),
        Column::Seen => age_label(now - track.last_seen),
        Column::Vendor => vendor_label(ap).to_owned(),
    }
}

/// "now" for fresh sightings, otherwise seconds or minutes ago.
pub fn age_label(age_secs: f64) -> String {
    let age = age_secs.max(0.0);
    if age < 3.0 {
        "now".to_owned()
    } else if age < 120.0 {
        format!("{age:.0} s")
    } else {
        format!("{:.0} min", age / 60.0)
    }
}

/// Vendor name, or why there is none (private Wi-Fi addresses are common on phones).
pub fn vendor_label(ap: &openspot_wlan::AccessPoint) -> &'static str {
    match ap.bssid.vendor() {
        Some(v) => v,
        None if ap.bssid.is_locally_administered() => "(private address)",
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Tracker;
    use openspot_wlan::{AccessPoint, MacAddress, NetworkType};

    #[test]
    fn percent_matches_the_documented_mapping() {
        assert_eq!(rssi_to_percent(-100), 0);
        assert_eq!(rssi_to_percent(-75), 50);
        assert_eq!(rssi_to_percent(-50), 100);
        assert_eq!(rssi_to_percent(-60), 80);
        assert_eq!(rssi_to_percent(-30), 100, "stronger than the ceiling clamps");
        assert_eq!(rssi_to_percent(-120), 0, "weaker than the floor clamps");
        assert_eq!(rssi_to_percent(-99), 2);
    }

    #[test]
    fn keys_round_trip_and_are_unique() {
        let keys: BTreeSet<_> = Column::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(keys.len(), Column::ALL.len());
        for c in Column::ALL {
            assert_eq!(Column::from_key(c.key()), Some(c));
        }
        assert_eq!(Column::from_key("nope"), None);
    }

    #[test]
    fn column_set_defaults_and_persistence() {
        let d = ColumnSet::default();
        assert!(d.contains(Column::Percent) && d.contains(Column::Vendor));
        assert!(!d.contains(Column::Alias) && !d.contains(Column::Type));
        let mut c = d.clone();
        assert!(c.set(Column::Type, true));
        assert!(c.set(Column::Vendor, false));
        let restored = ColumnSet::from_keys(Some(&c.to_keys()));
        assert_eq!(restored, c);
        assert_eq!(ColumnSet::from_keys(None), d);
        assert_eq!(ColumnSet::from_keys(Some(&["junk".to_owned()])), d, "unknown keys fall back to defaults");
    }

    #[test]
    fn last_visible_column_cannot_be_hidden() {
        let mut c = ColumnSet::from_keys(Some(&["ssid".to_owned()]));
        assert!(!c.set(Column::Ssid, false));
        assert!(c.contains(Column::Ssid));
        assert!(c.set(Column::Bssid, true));
        assert!(c.set(Column::Ssid, false));
    }

    #[test]
    fn display_order_is_fixed_regardless_of_toggle_order() {
        let mut c = ColumnSet::from_keys(Some(&["vendor".to_owned()]));
        c.set(Column::Ssid, true);
        c.set(Column::Channel, true);
        assert_eq!(c.iter().collect::<Vec<_>>(), vec![Column::Ssid, Column::Channel, Column::Vendor]);
    }

    #[test]
    fn raw_values_reflect_the_ap() {
        let ap = AccessPoint::from_beacon("Home".into(), MacAddress([0, 0, 0, 0, 0, 1]), -60, 2_437_000, true, 100, vec![])
            .with_network_type(NetworkType::AdHoc);
        let mut t = Tracker::default();
        t.ingest(vec![ap], 10.0);
        let track = t.get(&MacAddress([0, 0, 0, 0, 0, 1])).unwrap();
        let mut aliases = Aliases::default();
        assert_eq!(raw_value(Column::Alias, track, &aliases, 10.0), "");
        aliases.set(&track.ap.bssid, "Lab");
        assert_eq!(raw_value(Column::Alias, track, &aliases, 10.0), "Lab");
        assert_eq!(raw_value(Column::Ssid, track, &aliases, 10.0), "Lab");
        assert_eq!(raw_value(Column::Percent, track, &aliases, 10.0), "80");
        assert_eq!(raw_value(Column::Type, track, &aliases, 10.0), "Ad-hoc");
        assert_eq!(raw_value(Column::Channel, track, &aliases, 10.0), "6");
        assert_eq!(raw_value(Column::FirstSeen, track, &aliases, 100.0), "90 s");
    }

    #[test]
    fn age_wording() {
        assert_eq!(age_label(0.5), "now");
        assert_eq!(age_label(-5.0), "now");
        assert_eq!(age_label(30.0), "30 s");
        assert_eq!(age_label(300.0), "5 min");
    }
}
