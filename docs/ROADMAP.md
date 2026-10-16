# OpenSpot roadmap: every feature, planned

Status: `done` · `partial` · `todo` · `research` (needs a feasibility check first).
IDs are stable; reference them in commits and issues (`feat(heatmap): SNR map [H-04]`).

## How we work (checkpoints)

- One **checkpoint = one commit** (or a short PR) that builds, passes `cargo test --workspace`
  and `cargo clippy --workspace --all-targets` with no new warnings.
- Conventional Commits: `feat|fix|docs|test|refactor|ci|chore(scope): summary [ID]`.
- Each checkpoint lists its feature IDs in the commit body and ticks them here.
- A **milestone** is a tagged release (`v0.N.0`) when all its checkpoints are done.
- Work happens on `develop`/feature branches; `main` only receives reviewed merges.
- Every feature needs: unit tests for pure logic, a golden/fixture test where output is visual,
  and a line in `docs/` if user-visible. Anything touching real radios is also checked
  against a known-good tool and the result noted in the PR (see Q-01).

## Feature plan

### Scanning (crate `openspot-wlan`, `openspot-ie`)
| ID | Feature | Status | Acceptance |
|---|---|---|---|
| S-01 | Windows scan via Native Wifi (scan, wait for notification, BSS list) | done | Lists APs on real hardware |
| S-02 | 802.11 IE parser: PHY a/b/g/n/ac/ax/be, security, width, center channel | done | Unit tests + real-scan spot check |
| S-03 | Multi-probe merge | done | Mean RSSI, latest IEs |
| S-04 | Hidden SSID handling (empty SSID, user label, optional probe) | done | Hidden BSSIDs shown and labelable |
| S-05 | Adapter hot-plug and power notifications | done (UNVERIFIED hot-plug on hardware) | Unplug/replug updates list without restart |
| S-06 | Current connection info (BSSID, tx/rx rate, RSSI) | done | Matches `netsh wlan show interfaces` |
| S-07 | Noise floor source | decided: no noise source on Windows (docs/NOISE_FLOOR.md) | Decide: driver stats, `WlanQueryInterface`, or estimate; needed by H-04/H-06 |
| S-08 | Vendor lookup from OUI (IEEE registry, bundled and updatable) | done | Vendor column filled for >95 % APs |
| S-09 | Friendly permission errors (location services, service disabled) | done | Clear message + fix hint |
| S-10 | Linux backend (`nl80211` via `neli`) | partial (UNVERIFIED on hardware) | `openspot scan` works on Ubuntu |
| S-11 | macOS backend (CoreWLAN) | partial (type-checked only, UNVERIFIED) | Same CLI output on macOS |
| S-12 | Android/iOS | decided: out of scope for 1.0 | Likely out of scope for 1.0 |

### Inspector (app)
| ID | Feature | Status |
|---|---|---|
| I-01 | Live table, sort, filter by text and band | done |
| I-02 | RSSI-over-time chart, selectable window | done |
| I-03 | 2.4/5/6 GHz channel charts with real width | done |
| I-04 | Group-by (SSID, channel, band, security, vendor) + ungrouped bucket | done |
| I-05 | Extra columns: vendor, alias, avg/min/max RSSI, %, first/last seen, type | done |
| I-06 | Aliases (per BSSID), import/export, app-wide vs project | done |
| I-07 | Custom AP groups | done |
| I-08 | Event log (scan, pause, resume, adapter change, errors) with erase-after setting | done |
| I-09 | Preferences: interval presets, chart window, stale removal | done |
| I-10 | Channel recommendation (least congested channel per band, width-aware) | done |
| I-11 | CSV export of live table | done |
| I-12 | Sound notifications | partial (Windows only) |

### Survey (crates `openspot-model`, `openspot-store`, app)
| ID | Feature | Status |
|---|---|---|
| V-01 | Project > zone > snapshot, SQLite project file | done |
| V-02 | Load PNG/JPEG plan; blank plan of given size | done |
| V-03 | Scale calibration by segment | done |
| V-04 | Calibration by rectangle (known width x height) | done |
| V-05 | Click-to-scan measurement, hover details, undo last point | done |
| V-06 | Survey path mode (click waypoints, auto-sample along the path) | done |
| V-07 | Drawing tools (selection, polyline, rectangle, text, thickness/font presets) to draw a plan | done |
| V-08 | PDF floor plans (render first page; `pdfium-render`) | done |
| V-09 | Data-point radius presets by environment; measurements per sample | done |
| V-10 | Hidden networks in surveys | done |
| V-11 | Network selection, groups, aliases in survey | done |
| V-12 | Manual AP position + automatic estimate from strongest readings | done |
| V-13 | General undo/redo stack | done (map loads and zone delete clear history; docs/UNDO.md) |
| V-14 | Merge snapshots | done |
| V-15 | Crop heatmap area; gray out unmeasured area | done |
| V-16 | Units (m, cm, ft, in) and zoom presets | done |
| V-17 | Buildings with multiple floors (zones grouped), floor ordering | done |
| V-18 | Project explorer (sort by name/created/changed/opened/zones), start page, recent files | done |
| V-19 | Rename/delete/duplicate zones and snapshots | done |
| V-20 | Auto-backup, crash-safe saves (write-then-rename) | done |
| V-21 | Sound on success/failure | done |
| V-22 | Marker/path/plan visibility toggles, AP marker sizes and titles | done |

### Heatmaps (crate `openspot-heatmap`)
| ID | Feature | Status |
|---|---|---|
| H-01 | Signal level (best or selected networks) | done |
| H-02 | Number of APs | done |
| H-03 | Secondary signal (second strongest) | done |
| H-04 | SNR | blocked (no noise source) |
| H-05 | Signal-to-interference (co-channel, width-overlap aware) | done |
| H-06 | Noise level | blocked (no noise source) |
| H-07 | Band coverage (2.4/5/6/mixed) | done |
| H-08 | PHY-mode coverage | done |
| H-09 | Per-AP coverage (one map per AP) | done |
| H-10 | Wireless transmit rate | done (own link only; docs/TX_RATE.md) |
| H-11 | HTTP download/upload speed | done (UNVERIFIED vs real servers) |
| H-12 | iperf TCP down/up | done (UNVERIFIED vs real servers) |
| H-13 | iperf UDP down/up and jitter | done (UNVERIFIED vs real servers) |
| H-14 | Troubleshooting view: good/acceptable/critical regions with thresholds | done |
| H-15 | Gradient presets (default, red-green, blue-red), custom range | done |
| H-16 | Vector isoline renderer (contours, scalable export) | done |
| H-17 | Smoothing strength (normal/strong/extra) | done |
| H-18 | Legend, hover popup of values at cursor | done |
| H-19 | Heatmap on a worker thread with cancel + progress | done |

### Active scan (crate `openspot-speedtest`)
| ID | Feature | Status |
|---|---|---|
| A-01 | Connect to chosen SSID(s) per point, restore prior connection (guard) | done (UNVERIFIED on hardware) |
| A-02 | HTTP download/upload tests (URL, size) | done |
| A-03 | iperf3 TCP down/up via system `iperf3` JSON | done (UNVERIFIED vs real iperf3) |
| A-04 | iperf3 UDP down/up, jitter, bitrate cap | done (UNVERIFIED vs real iperf3) |
| A-05 | Preferences: duration, delay, streams, window size, test connection | done |

### Planning / predictive (crate `openspot-predict`)
| ID | Feature | Status |
|---|---|---|
| P-01 | Walls/doors/windows/racks drawing on plan | done (UI UNVERIFIED on a display) |
| P-02 | Material presets + custom (absorption, reflection), versioned data file | done |
| P-03 | Virtual AP placement; model catalog; per-radio channel/width/power/height/angle | done (generic AP models only; UI UNVERIFIED) |
| P-04 | Antenna patterns (omni/directional), import | partial (JSON format + parametric pattern; no vendor formats) |
| P-05 | Multi-wall propagation (ITU-R P.1238 style) | done |
| P-06 | Floor slabs, cutouts, level alignment | done |
| P-07 | Calibrate model to real measurements (least squares) | done (calibrated model is session-only) |
| P-08 | Ray-traced mode (image method) | done (optional single-bounce mode) |
| P-09 | Suggest AP placement / channel plan | done |
| P-10 | All H-* heatmaps available for predicted data | done (UI UNVERIFIED; no SNR) |

### Reports and interchange (crate `openspot-export`)
| ID | Feature | Status |
|---|---|---|
| R-01 | CSV export of survey data | done |
| R-02 | PNG export of plan + heatmap + points | done |
| R-03 | PDF quick report | done |
| R-04 | PDF advanced report (zones, snapshots, visualizations, view options, titles) | done |
| R-05 | Branding: company, logo, client, location, dates | done |
| R-06 | Per-AP heatmap folder export | done |
| R-07 | Project export/import archive | done |
| R-08 | Read-only importer for legacy survey files the user owns | decided: out of scope |

### Platform, quality, community
| ID | Feature | Status |
|---|---|---|
| U-01 | Preferences window with persistence | done |
| U-02 | Localization (fluent) | partial (infrastructure + mode bar and Preferences migrated) |
| U-03 | Light/dark themes, high-contrast, keyboard navigation | done |
| U-04 | Windows installer + code signing, portable zip | partial (scripts UNVERIFIED on Windows) |
| U-05 | Update check (opt-in) | done (opt-in; repo URL placeholder) |
| U-06 | CI: test + clippy on Linux and Windows (`.github/workflows/ci.yml`) | done |
| U-07 | Release pipeline (GitHub Actions, artifacts per OS) | partial (UNVERIFIED on GitHub) |
| U-08 | Website/docs, screenshots, user guide | done (Planning guide page is a stub) |
| U-09 | LICENSE-MIT, LICENSE-APACHE, CONTRIBUTING, CLEAN_ROOM, issue templates | done |
| Q-01 | Real-world validation corpus: captured beacons + comparison vs a known tool on the same spots | partial (`openspot verify` vs netsh; no heatmap/prediction ground truth) |
| Q-02 | Fuzz the IE parser (`cargo-fuzz`) | done |
| Q-03 | Golden-image tests for heatmaps/PNG | done |
| Q-04 | Benchmarks for interpolation on 10k points | done |

## Milestones and checkpoints

**M0 Foundation** (done): S-01..S-03, I-01..I-03, V-01..V-03, V-05, H-01, H-02, R-01, R-02, U-06.

**M1 Inspector complete**
1. S-08 vendor lookup · 2. I-05 columns · 3. I-04 group-by · 4. I-06/I-07 aliases and groups ·
5. I-08 event log · 6. I-10 channel recommendation · 7. I-09/U-01 preferences ·
8. I-11 CSV · 9. S-04 hidden SSIDs · 10. S-05/S-06 adapter + connection info.

**M2 Survey core complete**
1. V-13 undo/redo · 2. V-16 units/zoom · 3. V-19 zone/snapshot management ·
4. V-20 safe saves + backup · 5. V-18 explorer/start page · 6. V-04 rectangle calibration ·
7. V-06 path mode · 8. V-09 radius/sample settings · 9. V-12 AP position estimate ·
10. V-14 merge · 11. V-08 PDF plans · 12. V-07 drawing tools · 13. V-17 buildings · 14. V-10/V-22/V-21.

**M3 Heatmap suite**
1. H-03 secondary signal · 2. H-07 band · 3. H-08 PHY · 4. H-10 tx rate · 5. H-05 SIR ·
6. H-09 per-AP · 7. H-14 troubleshooting · 8. H-15/H-17/H-18 appearance and legend ·
9. H-16 vector isolines · 10. H-19 background rendering · 11. S-07 noise research then H-04/H-06 · 12. V-15 crop/gray-out.

**M4 Active scan:** A-01 → A-02 → A-03 → A-04 → A-05 → H-11..H-13.

**M5 Reports:** R-03 → R-05 → R-04 → R-06 → R-07 → R-08 (if feasible).

**M6 Planning:** P-02 → P-01 → P-03 → P-05 → P-06 → P-10 → P-07 → P-04 → P-08/P-09 research.

**M7 Cross-platform:** S-10 Linux, S-11 macOS, U-07 release pipeline, U-04 installers.

**M8 1.0:** Q-01..Q-04 complete, U-02 localization, U-03 accessibility, U-08 docs, all P0 bugs closed.

## Prioritisation rule
1. Correctness of numbers (Q-01) beats new features.
2. Troubleshooting value (SIR, band, secondary signal, troubleshooting view) before cosmetics.
3. Anything needing paid or proprietary data (router catalogs, antenna files) is built from public
   datasheets, with provenance recorded.
