//! CSV export of the live Inspector table.

use std::collections::BTreeSet;

use crate::aliases::Aliases;
use crate::columns::{self, Column, ColumnSet};
use crate::csvio;
use crate::state::Track;

/// Spreadsheet programs only read UTF-8 correctly when the file starts with a byte order
/// mark, and SSIDs are often non-ASCII.
const UTF8_BOM: &str = "\u{feff}";

/// Columns written to the file: the visible ones plus alias and vendor, which identify a
/// network even when the user hid them on screen. Always in table order.
pub fn export_columns(visible: &ColumnSet) -> Vec<Column> {
    let always: BTreeSet<Column> = [Column::Alias, Column::Vendor].into();
    Column::ALL.into_iter().filter(|c| visible.contains(*c) || always.contains(c)).collect()
}

fn header(c: Column) -> &'static str {
    match c {
        Column::Ssid => "SSID",
        Column::Rssi => "RSSI (dBm)",
        Column::Percent => "Signal (%)",
        Column::Avg => "Avg RSSI (dBm)",
        Column::Min => "Min RSSI (dBm)",
        Column::Max => "Max RSSI (dBm)",
        Column::Channel => "Channel",
        Column::Width => "Width (MHz)",
        Column::FirstSeen => "First seen (s ago)",
        Column::Seen => "Last seen (s ago)",
        other => other.title(),
    }
}

/// Machine-friendly cell value: numbers stay plain numbers (no units), the SSID is the real
/// SSID (the alias has its own column), and ages are whole seconds.
fn cell(c: Column, track: &Track, aliases: &Aliases, now: f64) -> String {
    match c {
        Column::Ssid => track.ap.ssid.clone(),
        Column::Width => track.ap.width.map_or_else(String::new, |w| w.mhz().to_string()),
        Column::FirstSeen => format!("{:.0}", (now - track.first_seen).max(0.0)),
        Column::Seen => format!("{:.0}", (now - track.last_seen).max(0.0)),
        Column::Vendor => track.ap.bssid.vendor().unwrap_or_default().to_owned(),
        other => columns::raw_value(other, track, aliases, now),
    }
}

/// Renders `rows` (already filtered and sorted as on screen) as CSV text. Every cell passes the
/// formula guard because SSIDs and aliases come from outside.
pub fn to_csv(rows: &[&Track], visible: &ColumnSet, aliases: &Aliases, now: f64, delimiter: char) -> String {
    let cols = export_columns(visible);
    let mut out = String::from(UTF8_BOM);
    out.push_str(&csvio::write_record(&cols.iter().map(|c| header(*c)).collect::<Vec<_>>(), delimiter));
    for track in rows {
        let cells: Vec<String> = cols.iter().map(|c| csvio::neutralize_formula(&cell(*c, track, aliases, now)).into_owned()).collect();
        out.push_str(&csvio::write_record(&cells, delimiter));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csvio::ParseMode;
    use crate::state::{Filter, Sort, Tracker};
    use openspot_wlan::{AccessPoint, MacAddress};

    fn ap(last: u8, ssid: &str, rssi: i32, khz: u32) -> AccessPoint {
        AccessPoint::from_beacon(ssid.into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, khz, true, 100, vec![])
    }

    fn parse(text: &str, delimiter: char) -> Vec<Vec<String>> {
        csvio::parse(text, delimiter, ParseMode::Strict).unwrap().into_iter().map(|r| r.fields).collect()
    }

    #[test]
    fn export_columns_add_alias_and_vendor_but_keep_order() {
        let only_channel = ColumnSet::from_keys(Some(&["channel".to_owned(), "ssid".to_owned()]));
        assert_eq!(export_columns(&only_channel), vec![Column::Ssid, Column::Alias, Column::Channel, Column::Vendor]);
    }

    #[test]
    fn rows_follow_the_given_order_and_filter_not_the_whole_tracker() {
        let mut t = Tracker::default();
        t.ingest(vec![ap(1, "Weak", -80, 2_437_000), ap(2, "Strong", -40, 2_437_000), ap(3, "Five", -50, 5_180_000)], 0.0);
        let filter = Filter { ghz5: false, ..Filter::default() };
        let rows = t.view(&filter, Sort::default());
        let csv = to_csv(&rows, &ColumnSet::default(), &Aliases::default(), 10.0, ',');
        let table = parse(&csv, ',');
        let ssid_col = table[0].iter().position(|h| h == "SSID").unwrap();
        let ssids: Vec<_> = table[1..].iter().map(|r| r[ssid_col].as_str()).collect();
        assert_eq!(ssids, vec!["Strong", "Weak"], "5 GHz filtered out, strongest first");
    }

    #[test]
    fn values_are_plain_and_include_alias_and_vendor() {
        let mut t = Tracker::default();
        let vm = AccessPoint::from_beacon("Lab".into(), MacAddress([0x00, 0x50, 0x56, 0, 0, 1]), -60, 2_437_000, true, 100, vec![]);
        t.ingest(vec![vm.clone()], 0.0);
        let mut aliases = Aliases::default();
        aliases.set(&vm.bssid, "Rack 4");
        let rows = t.view(&Filter::default(), Sort::default());
        let table = parse(&to_csv(&rows, &ColumnSet::default(), &aliases, 30.0, ','), ',');
        let get = |name: &str| table[1][table[0].iter().position(|h| h == name).unwrap()].clone();
        assert_eq!(get("SSID"), "Lab", "raw SSID, alias is separate");
        assert_eq!(get("Alias"), "Rack 4");
        assert!(get("Vendor").to_lowercase().contains("vmware"));
        assert_eq!(get("RSSI (dBm)"), "-60", "negative numbers are not text-prefixed");
        assert_eq!(get("Signal (%)"), "80");
        assert_eq!(get("Last seen (s ago)"), "30");
        assert_eq!(get("Channel"), "6");
        assert_eq!(get("BSSID"), "00:50:56:00:00:01");
    }

    #[test]
    fn hostile_names_are_escaped_and_defused() {
        let mut t = Tracker::default();
        t.ingest(vec![ap(1, "=cmd|' /C calc'!A0", -50, 2_437_000), ap(2, "a,\"b\"\nc", -60, 2_437_000)], 0.0);
        let mut aliases = Aliases::default();
        aliases.set(&MacAddress([0, 0, 0, 0, 0, 1]), "@evil");
        let rows = t.view(&Filter::default(), Sort::default());
        let table = parse(&to_csv(&rows, &ColumnSet::default(), &aliases, 0.0, ','), ',');
        assert_eq!(table.len(), 3, "the newline in a quoted SSID does not create a record");
        let ssid = table[0].iter().position(|h| h == "SSID").unwrap();
        let alias = table[0].iter().position(|h| h == "Alias").unwrap();
        assert_eq!(table[1][ssid], "'=cmd|' /C calc'!A0");
        assert_eq!(table[1][alias], "'@evil");
        assert_eq!(table[2][ssid], "a,\"b\"\nc", "awkward but harmless text survives intact");
        for r in &table {
            assert_eq!(r.len(), table[0].len());
        }
    }

    #[test]
    fn delimiter_choice_and_bom() {
        let mut t = Tracker::default();
        t.ingest(vec![ap(1, "a;b", -50, 2_437_000)], 0.0);
        let rows = t.view(&Filter::default(), Sort::default());
        let csv = to_csv(&rows, &ColumnSet::default(), &Aliases::default(), 0.0, ';');
        assert!(csv.starts_with(UTF8_BOM));
        let table = parse(&csv, ';');
        let ssid = table[0].iter().position(|h| h == "SSID").unwrap();
        assert_eq!(table[1][ssid], "a;b", "a semicolon inside a cell is quoted when it is the delimiter");
        assert!(csv.contains("\"a;b\""));
    }

    #[test]
    fn empty_table_still_writes_the_header() {
        let csv = to_csv(&[], &ColumnSet::default(), &Aliases::default(), 0.0, ',');
        assert_eq!(parse(&csv, ',').len(), 1);
    }
}
