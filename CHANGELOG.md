# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project aims to follow [Semantic Versioning](https://semver.org/) once it reaches 1.0; until then, minor
versions may change file formats.

"Unverified" means implemented and covered by automated tests, but not checked on real hardware, a real server,
or by a person looking at the result. [docs/guide/11-limits.md](docs/guide/11-limits.md) lists those areas and
[docs/ROADMAP.md](docs/ROADMAP.md) tracks every feature by ID.

## [Unreleased]

Nothing yet beyond 0.1.0.

## [0.1.0] - not yet tagged

First version. A summary of the roadmap as it stands; no release has been published.

### Added

**Scanning**
- Windows scanning through the Native Wifi API, with multi-probe merging, hidden-network handling, the current connection
  (BSSID, rates, signal), and actionable errors for the common failures (location access off, WLAN service stopped,
  radio off). Verified on one Windows 11 machine with an Intel AX211. [S-01, S-03, S-04, S-06, S-09]
- 802.11 information-element parser: PHY generations a/b/g/n/ac/ax/be, security, channel width up to 320 MHz,
  centre channel. Fuzz target included. [S-02, Q-02]
- Vendor lookup from the IEEE registry (24-bit assignments only). [S-08]
- Linux (`nl80211`) and macOS (CoreWLAN) scanners. **Unverified on hardware**; the macOS one is type-checked only. [S-10, S-11]
- Adapter hot-plug and power notifications in the library; **not wired into the application**. [S-05]

**Inspector**
- Live table with sorting, filtering by text and band, 17 selectable columns, grouping (SSID, channel, band, security, vendor,
  custom groups), aliases with CSV import and export, signal-over-time chart, 2.4/5/6 GHz channel charts at real width,
  channel advisor, event log with export, table CSV export with spreadsheet-formula protection. [I-01 to I-11]
- Opt-in sound for new networks (Windows only). [I-12]

**Survey**
- Projects as a single SQLite file (`.ospot`) with zones, snapshots and buildings with floors; start page and explorer;
  safe saves with a `.bak` copy; timed autosave with recovery. [V-01, V-17 to V-20]
- Floor plans from PNG, JPEG or PDF, a blank plan of a given size, or plans drawn in the application. [V-02, V-07, V-08]
- Scale calibration by segment or by rectangle; units m, cm, ft, in; zoom presets. [V-03, V-04, V-16]
- Click-to-measure, path mode that samples along a walked route, measurements per point (power-averaged), data point
  radius presets by environment, undo and redo of points, scale and merges. [V-05, V-06, V-09, V-13 partly]
- Access point position estimate with manual placement, merge of snapshots, crop and gray-out of unmeasured areas,
  visibility toggles, hidden networks in surveys. [V-10, V-12, V-14, V-15, V-22]

**Heatmaps**
- Signal level (best or selected networks), number of access points, secondary signal, co-channel signal-to-interference,
  band coverage, PHY coverage, per-AP coverage, and a troubleshooting view with editable thresholds. [H-01 to H-03, H-05, H-07 to H-09, H-14]
- Colour presets, custom value range, smoothing strength, legend with hover values, vector isolines (SVG export),
  rendering on a worker thread with progress and cancel. [H-15 to H-19]
- Throughput and jitter maps for HTTP and iperf3 tests (**unverified** against real servers). [H-11 to H-13]

**Active scan**
- HTTP download and upload, iperf3 TCP and UDP through the system `iperf3`, optional switching between saved Wi-Fi
  networks with restoration of the original connection. **Unverified** on real servers and hardware. [A-01 to A-05]

**Reports and interchange**
- CSV and PNG export; PDF quick and advanced reports with branding, per-AP heatmap images, and project archives
  (`.ospotz`, checksummed, strict import). PDF structure is tested; **page appearance is unverified**. [R-01 to R-07]

**Planning engine (no interface yet)**
- Material catalogue derived from ITU-R P.2040, multi-wall propagation in the style of ITU-R P.1238, floors and cutouts,
  calibration to measurements, optional single-bounce ray mode, access point placement suggestions, antenna pattern
  format. The Planning screens are not part of this version of the changelog. [P-02, P-05 to P-09, P-03/P-04/P-10 partly]

**Application and project**
- Preferences window with persistence, light, dark and high-contrast themes, interface size, keyboard navigation. [U-01, U-03]
- Opt-in update check (the repository address is still a placeholder). [U-05]
- Localization infrastructure (Fluent) with a pseudo-locale and a language selector; only the mode bar and the
  Preferences window are translated, and only English ships. [U-02 partly]
- User guide with reproducible pictures from synthetic data. [U-08]
- Windows installer and portable-zip scripts, **unverified**; Linux AppImage and macOS bundle sketches, experimental. [U-04 partly]
- CI on Linux and Windows; a release workflow that has never run. [U-06, U-07 partly]
- Benchmarks for interpolation on 10,000 points and golden-image tests for heatmaps and PNG export. [Q-03, Q-04]

### Not in this version

- SNR and noise maps: Windows exposes no noise value (docs/NOISE_FLOOR.md). [S-07, H-04, H-06]
- Wireless transmit rate map: the rate is not recorded per point. [H-10]
- Real-world validation corpus. [Q-01]
- Android and iOS. [S-12]
- Reading survey files from other products. [R-08]
