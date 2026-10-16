# 1. Getting started

## What you need

- A computer with a Wi-Fi adapter. Scanning needs real hardware; there is no radio in a virtual
  machine or in WSL.
- **Windows 10 or 11** is the only platform where OpenSpot has been run against a real adapter
  (one Intel AX211 on Windows 11). Linux and macOS backends exist but are **unverified on
  hardware**; see [Limits](11-limits.md).
- For the active scan: optionally `iperf3` (see [Active scan](05-active-scan.md)).

## Install

### Release downloads

Pushing a version tag runs `.github/workflows/release.yml`, which builds Windows, Linux and macOS
archives and attaches them, with a `SHA256SUMS` file, to a GitHub release. **That pipeline has never
run on GitHub** (it is listed as unverified in [Limits](11-limits.md)), and no release has been
published from this repository yet. Until one exists, build from source.

A Windows installer and a portable zip are prepared under `packaging/windows/`; they have not been
built on Windows either. See [packaging/README.md](../../packaging/README.md).

### Build from source

You need Rust 1.88 or newer (edition 2024).

```sh
git clone <repository url>
cd openspot
cargo build --release -p openspot-cli -p openspot-app
```

This produces two programs in `target/release/`:

| Program | What it is |
|---|---|
| `openspot-gui` | The desktop application: Inspector and Survey |
| `openspot` | Command-line scanner (`adapters`, `scan`, `status`, `events`) |

On Linux, building the GUI needs the GTK, xkbcommon, Wayland and X11 development packages, for
example on Debian or Ubuntu:

```sh
sudo apt-get install -y libgtk-3-dev libxkbcommon-dev libwayland-dev libx11-dev
```

Cross-compiling a Windows build from Linux or WSL:

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu -p openspot-app
```

## First run

```sh
openspot-gui            # real Wi-Fi
openspot-gui --fake     # built-in demo networks; works on any OS, no radio needed
```

`--fake` is the quickest way to explore the interface. It lists a few invented networks and
changes their signal over time. Surveys made with it contain invented data.

The window has two modes, chosen in the bar at the top: **Inspector** (live view of every network
in range) and **Survey** (measure signal on a floor plan and draw heatmaps). Preferences are behind
the **Preferences** button in the same bar.

If the application starts but shows "Wi-Fi is not available", it could not open a Wi-Fi backend or
find an adapter; the message under the heading says why.

The command-line scanner is useful to check that scanning works before blaming the GUI:

```sh
openspot adapters
openspot scan --probes 2
openspot scan --json
openspot --fake scan          # demo data
```

## Windows: location access

Starting with Windows 11 24H2, the operating system returns **empty or connected-network-only scan
results** to desktop apps unless location access is enabled. OpenSpot cannot tell this apart from
"no networks nearby", so it names the usual cause instead of guessing:

1. Open **Settings > Privacy & security > Location**.
2. Turn on **Location services**.
3. Turn on **Let desktop apps access your location**.
4. Restart OpenSpot or press **Scan now**.

OpenSpot does not use your location. Windows gates the list of nearby Wi-Fi networks behind this
setting because the list could be used to work out where you are. OpenSpot reads the list, shows it
on your screen, and keeps it on your computer; see Privacy in [Preferences and files](07-preferences-and-files.md).

If Windows answers "access denied", OpenSpot reports that and shows the same instructions. If the
**WLAN AutoConfig** service is stopped, run `net start wlansvc` from an elevated prompt.
Other causes of an empty list are in [Troubleshooting](09-troubleshooting.md).

## Next steps

- Looking around: [Inspector](02-inspector.md).
- Measuring a building: [Survey workflow](03-survey.md).
