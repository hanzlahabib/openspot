# OpenSpot user guide

OpenSpot is an open-source Wi-Fi analyzer and site-survey tool. This guide describes what the
application does **today**. Where something has not been verified on real hardware, or does not
exist yet, the page says so; the full list is in [Limits and known unverified areas](11-limits.md).

| Page | What it covers |
|---|---|
| [1. Getting started](01-getting-started.md) | Build or install, first run, Windows location access |
| [2. Inspector](02-inspector.md) | Live network table, charts, groups, aliases, event log, channel advisor, CSV |
| [3. Survey workflow](03-survey.md) | From floor plan to measured heatmap, step by step |
| [4. Heatmaps explained](04-heatmaps.md) | What each map means, how it is computed, default thresholds |
| [5. Active scan](05-active-scan.md) | Speed tests at each point: HTTP and iperf3 |
| [6. Reports and archives](06-reports.md) | PDF reports, per-AP images, project archives |
| [7. Preferences and files](07-preferences-and-files.md) | Settings, file types, where things are stored |
| [8. Keyboard and mouse](08-shortcuts.md) | Every shortcut the application has |
| [9. Troubleshooting and FAQ](09-troubleshooting.md) | No networks listed, permission errors, 6 GHz, missing SNR maps |
| [10. Planning mode](10-planning.md) | Coming in this build |
| [11. Limits and known unverified areas](11-limits.md) | What is not finished or not verified |

## Pictures in this guide

Every heatmap picture here is rendered from **synthetic data**: the sample floor plan
(`samples/sample-house-16x10m.png`), three invented access points and a simple distance-and-walls
signal model. Nothing was measured. They exist to show what a map looks like and how to read it,
not to suggest what a real house measures. Regenerate them with:

```sh
cargo run -p openspot-export --example guide_images
```

The program lives in `crates/openspot-export/examples/guide_images.rs`. Its output is
reproducible, so a change in a picture after regenerating shows a change in rendering.

OpenSpot is an independent implementation built from public specifications and public operating
system APIs; see [CLEAN_ROOM.md](../CLEAN_ROOM.md).
