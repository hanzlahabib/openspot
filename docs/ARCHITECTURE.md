# OpenSpot architecture (proposed)

## Crate map

| Crate | Role |
|---|---|
| `openspot-wlan` | `WlanProvider` trait; Windows (Native Wifi), Linux (nl80211) and macOS (CoreWLAN) backends; `fake` for tests |
| `openspot-ie` | Pure-Rust 802.11 information-element parser (PHY, security, channel width) |
| `openspot-model` | Domain model: projects, zones, snapshots, scale, planning structures |
| `openspot-store` | SQLite persistence with ordered migrations |
| `openspot-heatmap` | Triangulation, interpolation, isolines, rasterisation |
| `openspot-predict` | RF prediction (multi-wall and ITU-R P.2040 materials) |
| `openspot-speedtest` | iperf3 and HTTP throughput tests |
| `openspot-report` | PDF reports and `.ospotz` archives |
| `openspot-export` | CSV, PNG, SVG export |
| `openspot-i18n` | Fluent-based localisation |
| `openspot-cli`, `openspot-app` | Command line and egui desktop app |

## Layering

```
openspot-app  ──►  openspot-scan ─► openspot-wlan, openspot-ie, openspot-speedtest
      │                 │
      ├──► openspot-heatmap ◄── openspot-model ◄── openspot-store
      ├──► openspot-predict ◄──┘
      └──► openspot-report
```

Core crates are UI-free and `#![forbid(unsafe_code)]` except `openspot-wlan`
backends (FFI). Everything long-running takes a `CancellationToken` and
reports progress through a channel.

## Key design decisions

- **Provider trait over OS APIs** so Linux/macOS are additions, not rewrites.
- **Keep raw IE bytes** in every sample; the parser is a pure function.
- **Vector + raster heatmap paths** share one interpolation core.
- **Predictive engine is pluggable** (`trait Propagation`), so the simple
  multi-wall model ships first and ray tracing arrives later.
- **Data-driven catalogs** (materials, antennas, router models) as versioned
  TOML/JSON with provenance notes.

## Suggested roadmap

1. `openspot-wlan` Windows backend + CLI `openspot scan` (JSON out).
2. `openspot-ie` with test vectors captured from real beacons.
3. App shell + Inspector (live table, RSSI-over-time, channel graph).
4. Model + store + map import + calibration + Survey click-to-scan.
5. Heatmaps (signal, SNR, SIR, AP count, band) then speed tests.
6. Reports (PDF/CSV).
7. Planning mode (walls, APs, multi-wall prediction), then calibration.
8. Linux `nl80211`, macOS CoreWLAN.

## Licensing

MIT OR Apache-2.0. Dependencies must be compatible (note: iperf3 is BSD,
used as an external process only).
