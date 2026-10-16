#![forbid(unsafe_code)]

mod verify;

use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use openspot_wlan::{
    AccessPoint, Adapter, Connection, EMPTY_SCAN_HINT, FakeProvider, ScanOptions, WlanError,
    WlanEvent, WlanProvider, native_provider,
};

#[derive(Parser)]
#[command(name = "openspot", version, about = "Open-source Wi-Fi analyzer")]
struct Cli {
    /// Use the built-in fake provider instead of the OS Wi-Fi stack.
    #[arg(long, global = true)]
    fake: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List wireless adapters.
    Adapters,
    /// Scan for access points.
    Scan {
        /// Adapter id or description substring (default: first adapter).
        #[arg(long)]
        adapter: Option<String>,
        /// Probes merged into one result.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=10))]
        probes: u8,
        /// Seconds to wait for each probe.
        #[arg(long, default_value_t = 8)]
        timeout: u64,
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Show the network an adapter is connected to.
    Status {
        /// Adapter id or description substring (default: first adapter).
        #[arg(long)]
        adapter: Option<String>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Compare OpenSpot's scan with Windows' own `netsh wlan show networks` (read-only).
    Verify(verify::VerifyArgs),
    /// Print adapter arrival/removal and radio on/off events as they happen.
    Events {
        /// Stop after this many seconds.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
}

/// Everything a command can fail with.
#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Wlan(#[from] WlanError),
    #[error(transparent)]
    Verify(#[from] verify::VerifyError),
}

impl CliError {
    fn hint(&self) -> Option<String> {
        match self {
            Self::Wlan(e) => e.hint().map(str::to_owned),
            Self::Verify(e) => e.hint(),
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            if let Some(hint) = e.hint() {
                eprintln!("hint: {hint}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, CliError> {
    let provider: Box<dyn WlanProvider> = if cli.fake {
        Box::new(FakeProvider)
    } else {
        native_provider()?
    };

    match cli.command {
        Command::Adapters => {
            for a in provider.adapters()? {
                println!("{}  {}", a.id, a.description);
            }
        }
        Command::Scan {
            adapter,
            probes,
            timeout,
            json,
        } => {
            let chosen = choose_adapter(provider.as_ref(), adapter)?;
            let chosen = &chosen;
            let options = ScanOptions {
                probes,
                probe_timeout: Duration::from_secs(timeout),
            };
            let aps = provider.scan(chosen, &options)?;
            if aps.is_empty() {
                eprintln!("hint: {EMPTY_SCAN_HINT}");
            }
            if json {
                match serde_json::to_string_pretty(&aps) {
                    Ok(s) => println!("{s}"),
                    Err(e) => eprintln!("error: could not serialize results: {e}"),
                }
            } else {
                print_table(&aps);
            }
        }
        Command::Status { adapter, json } => {
            let chosen = choose_adapter(provider.as_ref(), adapter)?;
            let connection = provider.current_connection(&chosen)?;
            if json {
                match serde_json::to_string_pretty(&connection) {
                    Ok(s) => println!("{s}"),
                    Err(e) => eprintln!("error: could not serialize results: {e}"),
                }
            } else {
                print_status(&chosen, connection.as_ref());
            }
        }
        Command::Verify(args) => return Ok(verify::run(provider.as_ref(), &args)?),
        Command::Events { seconds } => {
            let stream = provider.subscribe_events()?;
            let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
            while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
                match stream.recv_timeout(left) {
                    Ok(WlanEvent::AdapterArrived { id }) => println!("adapter arrived: {id}"),
                    Ok(WlanEvent::AdapterRemoved { id }) => println!("adapter removed: {id}"),
                    Ok(WlanEvent::RadioChanged { id, on }) => {
                        println!("radio {}: {id}", if on { "on" } else { "off" });
                    }
                    Err(_) => break,
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn choose_adapter(
    provider: &dyn WlanProvider,
    query: Option<String>,
) -> Result<Adapter, WlanError> {
    let adapters = provider.adapters()?;
    let chosen = match &query {
        None => adapters.first(),
        Some(q) => adapters
            .iter()
            .find(|a| a.id.eq_ignore_ascii_case(q) || a.description.contains(q.as_str())),
    };
    chosen
        .cloned()
        .ok_or_else(|| query.map_or(WlanError::NoAdapters, WlanError::AdapterNotFound))
}

fn print_status(adapter: &Adapter, connection: Option<&Connection>) {
    println!("Adapter:  {}", adapter.description);
    let Some(c) = connection else {
        println!("State:    disconnected");
        return;
    };
    let ssid = if c.ssid.is_empty() {
        "<hidden>"
    } else {
        c.ssid.as_str()
    };
    println!("State:    {}", c.state.label());
    println!("SSID:     {ssid}");
    println!("BSSID:    {}", c.bssid);
    if let Some(phy) = c.phy {
        println!("Radio:    802.11{}", phy.label());
    }
    if let Some(ch) = c.channel {
        println!("Channel:  {ch}");
    }
    println!("Receive:  {} Mbps", c.rx_rate_mbps);
    println!("Transmit: {} Mbps", c.tx_rate_mbps);
    println!("Signal:   {}% ({} dBm)", c.signal_quality, c.rssi_dbm);
}

fn print_table(aps: &[AccessPoint]) {
    println!(
        "{:<28} {:<17} {:>5} {:>4} {:<7} {:<8} {:<12} Security",
        "SSID", "BSSID", "dBm", "Ch", "Band", "Width", "PHY"
    );
    for ap in aps {
        let ssid = if ap.ssid.is_empty() {
            "<hidden>"
        } else {
            ap.ssid.as_str()
        };
        let band = ap.band.map_or("?", |b| b.label());
        let width = ap.width.map_or("-".into(), |w| format!("{} MHz", w.mhz()));
        let phy = ap
            .phy
            .iter()
            .map(|p| p.label())
            .collect::<Vec<_>>()
            .join("/");
        let sec = ap
            .security
            .iter()
            .map(|s| s.label())
            .collect::<Vec<_>>()
            .join(" + ");
        println!(
            "{ssid:<28} {:<17} {:>5} {:>4} {band:<7} {width:<8} {phy:<12} {sec}",
            ap.bssid, ap.rssi_dbm, ap.channel
        );
    }
}
