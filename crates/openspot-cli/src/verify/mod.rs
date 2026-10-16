//! `openspot verify`: compares OpenSpot's scan with Windows' own view of the same networks
//! (`netsh wlan show networks mode=bssid`) and reports the differences (Q-01).
//!
//! Read-only: it scans like `openspot scan` does and runs `netsh wlan show`, nothing that
//! changes the Wi-Fi configuration. By default the report contains only counts and
//! statistics, no names or addresses, so it can be pasted into an issue or a document.

mod compare;
mod netsh;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Args;
use openspot_wlan::{Adapter, ScanOptions, WlanError, WlanProvider};
use serde_json::json;
use thiserror::Error;

use compare::{Agreement, Comparison, FieldTally, SIGNAL_TOLERANCE_DB, Summary, compare, summarize};
use netsh::{NetshError, NetshReport};

/// Probes merged for the native scan: a single probe often misses APs, and a missed AP would
/// be reported as "only in netsh" although nothing is wrong with the decoding.
const DEFAULT_PROBES: u8 = 3;

#[derive(Args)]
pub struct VerifyArgs {
    /// Adapter id or description substring (default: first adapter).
    #[arg(long)]
    pub adapter: Option<String>,
    /// Probes merged into the native scan.
    #[arg(long, default_value_t = DEFAULT_PROBES, value_parser = clap::value_parser!(u8).range(1..=10))]
    pub probes: u8,
    /// Seconds to wait for each probe.
    #[arg(long, default_value_t = 8)]
    pub timeout: u64,
    /// Read the netsh output from this file instead of running netsh (also the way to use
    /// this command away from Windows).
    #[arg(long)]
    pub netsh_file: Option<PathBuf>,
    /// List every network's differences. Prints BSSIDs, so do not paste it publicly.
    #[arg(long)]
    pub details: bool,
    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,
    /// Exit with status 3 if any field differs or any signal is outside the tolerance.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Error)]
pub enum VerifyError {
    #[error(transparent)]
    Wlan(#[from] WlanError),
    #[error(transparent)]
    Netsh(#[from] NetshError),
    #[error("could not run netsh: {0}")]
    NetshRun(String),
    #[error("could not read {path}: {reason}")]
    ReadFile { path: String, reason: String },
}

impl VerifyError {
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::Wlan(e) => e.hint().map(str::to_owned),
            Self::NetshRun(_) => Some("netsh is only available on Windows; save its output and pass --netsh-file".to_owned()),
            _ => None,
        }
    }
}

/// Exit status when `--strict` finds a difference.
const EXIT_DISAGREEMENT: u8 = 3;

pub fn run(provider: &dyn WlanProvider, args: &VerifyArgs) -> Result<ExitCode, VerifyError> {
    let adapter = choose_adapter(provider, args.adapter.as_deref())?;
    let options = ScanOptions { probes: args.probes, probe_timeout: Duration::from_secs(args.timeout) };
    // Scan first so that netsh reads the BSS list this scan just refreshed.
    let native = provider.scan(&adapter, &options)?;
    let text = match &args.netsh_file {
        Some(path) => std::fs::read_to_string(path).map_err(|e| VerifyError::ReadFile { path: path.display().to_string(), reason: e.to_string() })?,
        None => run_netsh()?,
    };
    let report = netsh::parse(&text)?;
    let comparison = compare(&native, &report);
    let summary = summarize(&comparison);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&to_json(&summary, &report, args.details.then_some(&comparison))).unwrap_or_default());
    } else {
        print!("{}", render(&adapter, &summary, &report));
        if args.details {
            print!("{}", render_details(&comparison));
        }
    }
    Ok(if args.strict && has_disagreement(&summary) { ExitCode::from(EXIT_DISAGREEMENT) } else { ExitCode::SUCCESS })
}

fn choose_adapter(provider: &dyn WlanProvider, query: Option<&str>) -> Result<Adapter, WlanError> {
    let adapters = provider.adapters()?;
    let chosen = match query {
        None => adapters.first(),
        Some(q) => adapters.iter().find(|a| a.id.eq_ignore_ascii_case(q) || a.description.contains(q)),
    };
    chosen.cloned().ok_or_else(|| query.map_or(WlanError::NoAdapters, |q| WlanError::AdapterNotFound(q.to_owned())))
}

#[cfg(windows)]
fn run_netsh() -> Result<String, VerifyError> {
    let output = std::process::Command::new("netsh")
        .args(["wlan", "show", "networks", "mode=bssid"])
        .output()
        .map_err(|e| VerifyError::NetshRun(e.to_string()))?;
    if !output.status.success() {
        return Err(VerifyError::NetshRun(format!("netsh exited with {}", output.status)));
    }
    // The console code page is not UTF-8; only the ASCII structure matters and names are
    // never compared, so replacing undecodable bytes is harmless.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(windows))]
fn run_netsh() -> Result<String, VerifyError> {
    Err(VerifyError::NetshRun("this is not Windows".to_owned()))
}

/// Whether anything differs: a mismatching field or a signal outside the tolerance. Networks
/// seen by only one side are reported but do not count, because the two scans are taken
/// moments apart and weak APs come and go.
fn has_disagreement(s: &Summary) -> bool {
    [s.channel, s.band, s.phy].iter().any(|t| t.mismatches > 0) || s.signal.within_tolerance < s.signal.compared
}

fn tally_line(name: &str, t: FieldTally) -> String {
    format!("{name:<8}{} match, {} differ, {} not comparable\n", t.matches, t.mismatches, t.not_comparable)
}

fn render(adapter: &Adapter, s: &Summary, report: &NetshReport) -> String {
    let mut out = format!("Adapter: {}\n", adapter.description);
    out.push_str(&format!(
        "Networks: OpenSpot {}, netsh {}, matched by BSSID {}, only in OpenSpot {}, only in netsh {}\n",
        s.native_count, s.netsh_count, s.matched, s.only_native, s.only_netsh
    ));
    out.push_str(&tally_line("Channel", s.channel));
    out.push_str(&tally_line("Band", s.band));
    out.push_str(&tally_line("Radio", s.phy));
    let g = &s.signal;
    if let (Some(mean_abs), Some(max_abs), Some(mean_signed)) = (g.mean_abs_db, g.max_abs_db, g.mean_signed_db) {
        out.push_str(&format!(
            "Signal  {} compared (netsh % -> dBm as -100 + %/2, RSSI limited to -100..-50 dBm): mean |diff| {mean_abs:.1} dB, max |diff| {max_abs} dB, mean signed {mean_signed:+.1} dB, {} within {SIGNAL_TOLERANCE_DB} dB\n",
            g.compared, g.within_tolerance
        ));
    } else {
        out.push_str("Signal  nothing to compare\n");
    }
    if report.incomplete > 0 {
        out.push_str(&format!("Note: {} netsh entries lacked one or more fields and were compared on the rest.\n", report.incomplete));
    }
    out.push_str("Both sides read the same Windows BSS list: this checks decoding, not the radio. See docs/VALIDATION.md.\n");
    out
}

fn render_details(c: &Comparison) -> String {
    let mut out = String::from("\nPer network (BSSIDs shown):\n");
    for d in &c.matched {
        let signal = d.signal.as_ref().map_or_else(
            || "signal n/a".to_owned(),
            |s| {
                let clamped = if s.native_in_range_dbm == s.native_dbm { String::new() } else { format!(", compared as {}", s.native_in_range_dbm) };
                format!("signal {} dBm{clamped} vs {}% = {} dBm ({:+} dB)", s.native_dbm, s.netsh_percent, s.netsh_dbm, s.delta_db)
            },
        );
        out.push_str(&format!("{}  ch {}  band {}  radio {}  {signal}\n", d.bssid, field(&d.channel), field(&d.band), field(&d.phy)));
    }
    for b in &c.only_native {
        out.push_str(&format!("{b}  only in OpenSpot\n"));
    }
    for b in &c.only_netsh {
        out.push_str(&format!("{b}  only in netsh\n"));
    }
    out
}

fn field(a: &Agreement) -> String {
    match a {
        Agreement::Match => "ok".to_owned(),
        Agreement::Mismatch { native, netsh } => format!("DIFF (OpenSpot {native}, netsh {netsh})"),
        Agreement::NotComparable => "n/a".to_owned(),
    }
}

fn tally_json(t: FieldTally) -> serde_json::Value {
    json!({ "match": t.matches, "differ": t.mismatches, "not_comparable": t.not_comparable })
}

fn to_json(s: &Summary, report: &NetshReport, details: Option<&Comparison>) -> serde_json::Value {
    let mut value = json!({
        "networks": {
            "openspot": s.native_count, "netsh": s.netsh_count, "matched": s.matched,
            "only_openspot": s.only_native, "only_netsh": s.only_netsh,
        },
        "channel": tally_json(s.channel),
        "band": tally_json(s.band),
        "radio": tally_json(s.phy),
        "signal": {
            "compared": s.signal.compared,
            "within_tolerance": s.signal.within_tolerance,
            "tolerance_db": SIGNAL_TOLERANCE_DB,
            "mean_abs_db": s.signal.mean_abs_db,
            "max_abs_db": s.signal.max_abs_db,
            "mean_signed_db": s.signal.mean_signed_db,
        },
        "netsh_entries_incomplete": report.incomplete,
    });
    if let Some(c) = details {
        value["details"] = c
            .matched
            .iter()
            .map(|d| {
                json!({
                    "bssid": d.bssid.to_string(),
                    "channel": field(&d.channel), "band": field(&d.band), "radio": field(&d.phy),
                    "signal_delta_db": d.signal.as_ref().map(|s| s.delta_db),
                })
            })
            .collect();
    }
    value
}

#[cfg(test)]
mod tests {
    use openspot_wlan::{AccessPoint, FakeProvider, MacAddress};

    use super::*;

    fn summary_for(native: &[AccessPoint], netsh_text: &str) -> (Summary, NetshReport) {
        let report = netsh::parse(netsh_text).unwrap();
        (summarize(&compare(native, &report)), report)
    }

    fn netsh_for(aps: &[AccessPoint], percent: u8) -> String {
        let mut text = String::from("Interface name : Wi-Fi\r\nThere are networks currently visible.\r\n\r\n");
        for (i, ap) in aps.iter().enumerate() {
            let phy = ap.phy.iter().next_back().map_or(String::new(), |p| format!("802.11{}", p.label()));
            text.push_str(&format!(
                "SSID {} : x\r\n    BSSID 1 : {}\r\n         Signal : {percent}%\r\n         Radio type : {phy}\r\n         Band : {}\r\n         Channel : {}\r\n",
                i + 1,
                ap.bssid.to_string().to_lowercase(),
                ap.band.map_or("", |b| b.label()),
                ap.channel
            ));
        }
        text
    }

    #[test]
    fn the_fake_providers_networks_agree_with_netsh_text_built_from_them() {
        let fake = FakeProvider;
        let adapter = fake.adapters().unwrap().remove(0);
        let aps = fake.scan(&adapter, &ScanOptions::default()).unwrap();
        let (s, report) = summary_for(&aps, &netsh_for(&aps, 80));
        assert_eq!((s.matched, s.only_native, s.only_netsh), (3, 0, 0));
        for t in [s.channel, s.band, s.phy] {
            assert_eq!(t, FieldTally { matches: 3, mismatches: 0, not_comparable: 0 });
        }
        assert_eq!(report.incomplete, 0);
        let text = render(&adapter, &s, &report);
        assert!(text.contains("matched by BSSID 3") && text.contains("Channel"), "{text}");
        assert!(!text.contains("02:00:00"), "the default report must not print BSSIDs: {text}");
    }

    #[test]
    fn a_tampered_channel_is_caught_and_strict_mode_would_fail() {
        let fake = FakeProvider;
        let adapter = fake.adapters().unwrap().remove(0);
        let aps = fake.scan(&adapter, &ScanOptions::default()).unwrap();
        let text = netsh_for(&aps, 80).replacen("Channel : 36", "Channel : 40", 1);
        let (s, _) = summary_for(&aps, &text);
        assert_eq!(s.channel.mismatches, 1);
        assert!(has_disagreement(&s));
        let clean = summary_for(&aps, &netsh_for(&aps, 80)).0;
        // The fake RSSI values are far from -60 dBm for some APs, so only fields decide here.
        assert!(!has_disagreement(&Summary { signal: Default::default(), ..clean }));
    }

    #[test]
    fn signal_outside_the_tolerance_counts_as_disagreement() {
        let mut s = Summary::default();
        s.signal.compared = 2;
        s.signal.within_tolerance = 1;
        assert!(has_disagreement(&s));
        s.signal.within_tolerance = 2;
        assert!(!has_disagreement(&s));
    }

    #[test]
    fn json_report_has_the_counts_and_no_addresses_unless_details_are_requested() {
        let fake = FakeProvider;
        let adapter = fake.adapters().unwrap().remove(0);
        let aps = fake.scan(&adapter, &ScanOptions::default()).unwrap();
        let report = netsh::parse(&netsh_for(&aps, 80)).unwrap();
        let c = compare(&aps, &report);
        let s = summarize(&c);
        let plain = to_json(&s, &report, None);
        assert_eq!(plain["networks"]["matched"], 3);
        assert_eq!(plain["signal"]["tolerance_db"], SIGNAL_TOLERANCE_DB);
        assert!(!plain.to_string().contains("02:00:00"));
        let detailed = to_json(&s, &report, Some(&c));
        assert_eq!(detailed["details"].as_array().map(Vec::len), Some(3));
        assert!(detailed.to_string().contains("02:00:00:00:00:01"));
    }

    #[test]
    fn a_language_problem_surfaces_as_an_error_not_a_report() {
        let err = netsh::parse("SSID 1 : x\r\n    BSSID 1 : 02:00:00:00:00:01\r\n         Kanal : 36\r\n").unwrap_err();
        assert!(matches!(VerifyError::from(err), VerifyError::Netsh(NetshError::UnrecognisedLanguage { .. })));
    }

    #[test]
    fn unused_mac_helper_compiles_for_error_hints() {
        let e = VerifyError::NetshRun("x".into());
        assert!(e.hint().is_some());
        assert_eq!(MacAddress([0; 6]).to_string(), "00:00:00:00:00:00");
    }
}
