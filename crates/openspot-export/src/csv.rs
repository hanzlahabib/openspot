use std::fmt::Write;

use openspot_model::{Snapshot, Zone};

const HEADER: &str = "zone,snapshot,point,x_px,y_px,x_m,y_m,unix_time,ssid,bssid,rssi_dbm,channel,band,width_mhz,phy,security,connected_bssid,tx_rate_mbps,rx_rate_mbps";

/// RFC 4180 field quoting: wrap in quotes (doubling inner quotes) when the field holds a
/// delimiter, quote, or line break. Leading `= + - @` are neutralised so spreadsheets do
/// not evaluate an attacker-chosen SSID as a formula.
fn field(value: &str) -> String {
    let guarded = if value.starts_with(['=', '+', '-', '@']) { format!("'{value}") } else { value.to_owned() };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// One row per (scan point, access point). Meter columns are empty until the zone has a scale.
///
/// The last three columns describe the adapter's own connection when the point was measured
/// (the same on every row of a point); they are empty when it was not connected or the rate was
/// not reported, because an empty cell is "unknown" while 0 would claim a measured rate.
pub fn snapshot_to_csv(zone: &Zone, snapshot: &Snapshot) -> String {
    let mut out = String::from(HEADER);
    out.push_str("\r\n");
    for (i, point) in snapshot.points.iter().enumerate() {
        let (xm, ym) = match zone.scale {
            Some(s) => (format!("{:.2}", s.to_meters(point.x)), format!("{:.2}", s.to_meters(point.y))),
            None => (String::new(), String::new()),
        };
        let (link_bssid, tx, rx) = point.link.as_ref().map_or_else(Default::default, |l| {
            let rate = |mbps: f64| if mbps.is_finite() && mbps > 0.0 { format!("{mbps:.1}") } else { String::new() };
            (l.bssid.to_string(), rate(l.tx_rate_mbps), rate(l.rx_rate_mbps))
        });
        for ap in &point.access_points {
            let phy = ap.phy.iter().map(|p| p.label()).collect::<Vec<_>>().join("/");
            let sec = ap.security.iter().map(|s| s.label()).collect::<Vec<_>>().join(" + ");
            // Writing to a String cannot fail.
            let _ = write!(
                out,
                "{},{},{},{:.1},{:.1},{xm},{ym},{},{},{},{},{},{},{},{},{},{link_bssid},{tx},{rx}\r\n",
                field(&zone.name),
                field(&snapshot.name),
                i + 1,
                point.x,
                point.y,
                point.time,
                field(&ap.ssid),
                ap.bssid,
                ap.rssi_dbm,
                ap.channel,
                ap.band.map_or("", |b| b.label()),
                ap.width.map_or(String::new(), |w| w.mhz().to_string()),
                field(&phy),
                field(&sec),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::{ScanPoint, Scale};
    use openspot_wlan::{AccessPoint, MacAddress};

    fn ap(ssid: &str, rssi: i32) -> AccessPoint {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0xAA, 0, 0, 0, 0, 1]), rssi, 5_180_000, true, 100, vec![])
    }

    #[test]
    fn rows_header_and_scale_columns() {
        let mut zone = Zone::new("Ground, floor");
        zone.scale = Scale::new(10.0);
        let mut snap = Snapshot::new("S1");
        snap.points.push(ScanPoint::new(50.0, 25.0, 1700000000, vec![ap("Home", -48)]));
        let csv = snapshot_to_csv(&zone, &snap);
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(lines[0], HEADER);
        assert!(lines[1].starts_with("\"Ground, floor\",S1,1,50.0,25.0,5.00,2.50,1700000000,Home,AA:00:00:00:00:01,-48,36,5 GHz,"));
        assert_eq!(lines[2], "");
    }

    fn link(tx: f64, rx: f64) -> openspot_model::LinkInfo {
        openspot_model::LinkInfo { bssid: MacAddress([0xAA, 0, 0, 0, 0, 9]), tx_rate_mbps: tx, rx_rate_mbps: rx }
    }

    #[test]
    fn link_columns_repeat_per_row_and_are_empty_when_unknown() {
        let zone = Zone::new("z");
        let mut snap = Snapshot::new("s");
        let mut connected = ScanPoint::new(1.0, 1.0, 0, vec![ap("a", -50), ap("b", -60)]);
        connected.link = Some(link(866.7, 780.0));
        let mut no_rate = ScanPoint::new(2.0, 2.0, 0, vec![ap("c", -70)]);
        no_rate.link = Some(link(0.0, 0.0));
        snap.points.push(connected);
        snap.points.push(no_rate);
        snap.points.push(ScanPoint::new(3.0, 3.0, 0, vec![ap("d", -80)]));
        let csv = snapshot_to_csv(&zone, &snap);
        let rows: Vec<&str> = csv.split("\r\n").skip(1).filter(|l| !l.is_empty()).collect();
        assert_eq!(rows.len(), 4);
        assert!(rows[0].ends_with(",AA:00:00:00:00:09,866.7,780.0"), "{}", rows[0]);
        assert!(rows[1].ends_with(",AA:00:00:00:00:09,866.7,780.0"), "same link on every AP row of the point");
        assert!(rows[2].ends_with(",AA:00:00:00:00:09,,"), "connected without reported rates: empty, not 0: {}", rows[2]);
        assert!(rows[3].ends_with(",,,"), "never connected: all three empty: {}", rows[3]);
        assert!(csv.starts_with(HEADER) && HEADER.ends_with("connected_bssid,tx_rate_mbps,rx_rate_mbps"));
    }

    #[test]
    fn meters_are_empty_without_scale() {
        let zone = Zone::new("z");
        let mut snap = Snapshot::new("s");
        snap.points.push(ScanPoint::new(1.0, 2.0, 0, vec![ap("x", -60)]));
        assert!(snapshot_to_csv(&zone, &snap).contains(",1.0,2.0,,,0,x,"));
    }

    #[test]
    fn quoting_and_formula_guard() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("a\"b"), "\"a\"\"b\"");
        assert_eq!(field("line\nbreak"), "\"line\nbreak\"");
        assert_eq!(field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(field("-5"), "'-5");
    }

    #[test]
    fn empty_snapshot_is_header_only() {
        assert_eq!(snapshot_to_csv(&Zone::new("z"), &Snapshot::new("s")), format!("{HEADER}\r\n"));
    }
}
