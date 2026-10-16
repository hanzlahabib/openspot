//! Per-network figures shown in the report tables.

use std::collections::BTreeMap;

use openspot_model::Snapshot;
use openspot_wlan::{Band, ChannelWidth, MacAddress, SecurityMode};

/// One access point (BSSID), aggregated over the snapshots it is listed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRow {
    pub bssid: MacAddress,
    pub ssid: String,
    pub alias: Option<String>,
    pub channel: u16,
    pub band: Option<Band>,
    pub width: Option<ChannelWidth>,
    pub security: String,
    pub vendor: Option<&'static str>,
    /// Strongest reading anywhere; the figure all other attributes come from.
    pub max_rssi_dbm: i32,
    /// Measurement points that heard this AP.
    pub points_heard: usize,
}

impl NetworkRow {
    /// Alias, else SSID, else a placeholder for hidden networks. With an alias the SSID
    /// follows in parentheses so the real network stays recognisable.
    pub fn display_name(&self) -> String {
        match (&self.alias, self.ssid.is_empty()) {
            (Some(a), true) => a.clone(),
            (Some(a), false) => format!("{a} ({})", self.ssid),
            (None, true) => "<hidden>".to_owned(),
            (None, false) => self.ssid.clone(),
        }
    }

    pub fn band_label(&self) -> &'static str {
        self.band.map_or("-", Band::label)
    }

    pub fn width_label(&self) -> String {
        self.width.map_or_else(|| "-".to_owned(), |w| format!("{} MHz", w.mhz()))
    }
}

fn security_label(ap: &openspot_wlan::AccessPoint) -> String {
    let labels: Vec<&str> = ap.security.iter().map(|s: &SecurityMode| s.label()).collect();
    if labels.is_empty() { "-".to_owned() } else { labels.join(" / ") }
}

/// Aggregates every BSSID seen in `snapshots`, strongest first (ties by BSSID so the
/// order, and therefore the report, is deterministic).
pub fn network_rows<'a>(snapshots: impl IntoIterator<Item = &'a Snapshot>, aliases: &BTreeMap<MacAddress, String>) -> Vec<NetworkRow> {
    let mut rows: BTreeMap<MacAddress, NetworkRow> = BTreeMap::new();
    for ap in snapshots.into_iter().flat_map(|s| s.points.iter().flat_map(|p| &p.access_points)) {
        let row = rows.entry(ap.bssid).or_insert_with(|| NetworkRow {
            bssid: ap.bssid,
            ssid: ap.ssid.clone(),
            alias: aliases.get(&ap.bssid).map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()),
            channel: ap.channel,
            band: ap.band,
            width: ap.width,
            security: security_label(ap),
            vendor: ap.bssid.vendor(),
            max_rssi_dbm: i32::MIN,
            points_heard: 0,
        });
        row.points_heard += 1;
        if ap.rssi_dbm > row.max_rssi_dbm {
            row.max_rssi_dbm = ap.rssi_dbm;
            row.ssid.clone_from(&ap.ssid);
            row.channel = ap.channel;
            row.band = ap.band;
            row.width = ap.width;
            row.security = security_label(ap);
        }
    }
    let mut out: Vec<_> = rows.into_values().collect();
    out.sort_by(|a, b| b.max_rssi_dbm.cmp(&a.max_rssi_dbm).then(a.bssid.cmp(&b.bssid)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::ScanPoint;
    use openspot_wlan::AccessPoint;

    fn ap(last: u8, ssid: &str, rssi: i32, khz: u32) -> AccessPoint {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0, 0x1A, 0x2B, 0, 0, last]), rssi, khz, true, 100, vec![])
    }

    #[test]
    fn aggregates_strongest_sighting_and_counts_points() {
        let mut s1 = Snapshot::new("a");
        s1.points.push(ScanPoint::new(0.0, 0.0, 0, vec![ap(1, "old", -70, 2_437_000), ap(2, "b", -50, 5_180_000)]));
        let mut s2 = Snapshot::new("b");
        s2.points.push(ScanPoint::new(0.0, 0.0, 0, vec![ap(1, "new", -55, 2_462_000)]));
        let aliases = BTreeMap::from([(MacAddress([0, 0x1A, 0x2B, 0, 0, 1]), "  Lobby ".to_owned())]);
        let rows = network_rows([&s1, &s2], &aliases);
        assert_eq!(rows.len(), 2);
        // Strongest first: ap 2 at -50, then ap 1 at -55.
        assert_eq!((rows[0].bssid.0[5], rows[0].max_rssi_dbm), (2, -50));
        let r = &rows[1];
        assert_eq!((r.max_rssi_dbm, r.points_heard, r.ssid.as_str(), r.channel), (-55, 2, "new", 11));
        assert_eq!(r.display_name(), "Lobby (new)");
        assert_eq!(rows[0].band_label(), "5 GHz");
    }

    #[test]
    fn names_for_hidden_and_unaliased() {
        let mut s = Snapshot::new("a");
        s.points.push(ScanPoint::new(0.0, 0.0, 0, vec![ap(1, "", -60, 2_437_000), ap(2, "Home", -61, 2_437_000)]));
        let rows = network_rows([&s], &BTreeMap::new());
        assert_eq!(rows[0].display_name(), "<hidden>");
        assert_eq!(rows[1].display_name(), "Home");
    }

    #[test]
    fn equal_signal_orders_by_bssid() {
        let mut s = Snapshot::new("a");
        s.points.push(ScanPoint::new(0.0, 0.0, 0, vec![ap(9, "z", -60, 2_437_000), ap(3, "y", -60, 2_437_000)]));
        let rows = network_rows([&s], &BTreeMap::new());
        assert_eq!(rows.iter().map(|r| r.bssid.0[5]).collect::<Vec<_>>(), vec![3, 9]);
    }
}
