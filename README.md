# OpenSpot: open-source WiFi analyzer, site survey and heatmap tool

**OpenSpot is a free, open-source WiFi analyzer and WiFi site survey tool written in Rust.** Scan nearby networks,
walk a floor plan while measuring signal strength, and turn the measurements into a **WiFi heatmap** that shows
coverage, interference and dead zones. It also plans a network on paper before you install any hardware.

**A free, open-source NetSpot alternative** for Windows, Linux and macOS. No account, no licence key, no telemetry. Everything stays on your computer.

> **Status: pre-release (0.1.0).** It is built and tested on Windows against one real adapter. Linux and macOS
> scanning, real speed-test servers and the installer are implemented but **not yet verified on hardware**.
> The honest list is in [docs/guide/11-limits.md](docs/guide/11-limits.md). No binary release exists yet, so you
> build from source for now. **Testers with different hardware are the most valuable contribution right now.**

## Why OpenSpot

- **Free and open**: MIT OR Apache-2.0. Commercial WiFi survey tools charge per seat or per year; OpenSpot does not.
- **Private by design**: no telemetry, no cloud, no sign-up. The only network traffic is the opt-in update check and
  speed tests you configure.
- **Fast and small**: a native Rust app, not a web view. Core logic lives in UI-free crates you can reuse.
- **Honest about accuracy**: every feature is marked as verified or not. See [docs/VALIDATION.md](docs/VALIDATION.md).

## Features

| Area | What you get |
|---|---|
| **WiFi analyzer (Inspector)** | Live table of access points (17 columns), grouping, aliases, signal-over-time and **WiFi channel analyzer** charts, channel advisor, event log, CSV export |
| **802.11 decoding** | Pure-Rust information-element parser: PHY a/b/g/n/ac/ax/be, WPA/WPA2/WPA3/OWE, channel width up to 320 MHz |
| **WiFi site survey (Survey)** | PNG, JPEG or PDF floor plans (or a blank or drawn plan), scale calibration, click-to-measure and walked-path sampling, zones, snapshots, buildings, autosave |
| **WiFi heatmap** | Signal strength, secondary signal, signal-to-interference, band and PHY coverage, AP count, per-AP maps, troubleshooting view, isolines |
| **Speed maps** | HTTP and iperf3 throughput and jitter, drawn as maps (unverified against real servers) |
| **WiFi planner (Planning)** | Walls, materials, virtual APs, multi-wall and ITU-R P.2040 prediction, calibration, automatic AP placement |
| **Reports** | PDF reports with branding, CSV, PNG, SVG, and shareable `.ospotz` project archives |
| **CLI** | `openspot scan`, `openspot verify` and more, scriptable JSON output |

Feature-by-feature status with stable IDs: [docs/ROADMAP.md](docs/ROADMAP.md).

## Platforms

| Platform | Support |
|---|---|
| Windows 10 / 11 | **Verified** on one adapter (Intel AX211) |
| Linux (nl80211) | Builds and passes tests; scanning not yet run on hardware; needs `CAP_NET_ADMIN` |
| macOS (CoreWLAN) | Type-checked only; Location Services authorisation not requested yet |
| Android, iOS | Out of scope for 1.0 ([why](docs/DECISION_MOBILE.md)) |

## Build and run

Rust 1.88 or newer (edition 2024). On Linux the GUI also needs
`libgtk-3-dev libxkbcommon-dev libwayland-dev libx11-dev` (Debian and Ubuntu package names).

```sh
git clone https://github.com/hanzlahabib/openspot
cd openspot
cargo build --release -p openspot-cli -p openspot-app
target/release/openspot-gui            # desktop app
target/release/openspot-gui --fake     # demo networks, any OS, no radio needed
target/release/openspot scan           # command-line scan
```

Cross-compile for Windows from Linux or WSL:
`cargo build --release --target x86_64-pc-windows-gnu -p openspot-cli -p openspot-app`.

On Windows 11 24H2 and later, turn on **Location services** and **Let desktop apps access your location**, or scans
come back empty ([details](docs/guide/01-getting-started.md#windows-location-access)).

## Documentation

- **[User guide](docs/guide/README.md)**: install, Inspector, survey workflow, what each heatmap means and how it is computed,
  active scan, reports, preferences, shortcuts, troubleshooting, known limits
- [Roadmap](docs/ROADMAP.md) · [Architecture](docs/ARCHITECTURE.md) · [Testing](docs/TESTING.md) ·
  [Planning engine](docs/PLANNING_ENGINE.md) · [Reports](docs/REPORTS.md) · [Changelog](CHANGELOG.md)

## Contribute: help wanted

OpenSpot is young and needs people. You do not need to be a Rust expert.

- **Test on your hardware**: run `openspot verify` on Linux or macOS, or on another Windows adapter, and open an
  issue with the result. This is the single most useful thing right now.
- **Validate accuracy**: compare an OpenSpot survey with a known-good tool or a calibrated meter (roadmap item Q-01).
- **Take a roadmap item** marked partial in [docs/ROADMAP.md](docs/ROADMAP.md): Linux and macOS polish, translations,
  installer testing, noise-floor sources.
- **Translate** the interface: see [docs/I18N.md](docs/I18N.md).
- **Report bugs** and weird access-point beacons (synthetic or anonymised only).

Start with [CONTRIBUTING.md](CONTRIBUTING.md) and the [clean-room rules](docs/CLEAN_ROOM.md), and follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security problems privately as described in [SECURITY.md](SECURITY.md).

If OpenSpot is useful to you, a GitHub star helps others find it.

## Privacy

No telemetry, no analytics, no accounts. See [SECURITY.md](SECURITY.md).

## Independence

OpenSpot is an independent implementation built from public specifications (IEEE 802.11, ITU-R) and public operating-system
APIs. It is not affiliated with, endorsed by or sponsored by NetSpot or any other commercial product. All product names are trademarks of their respective owners and are used only to describe compatibility or comparison.

## Licence

MIT OR Apache-2.0, at your option: [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE).
