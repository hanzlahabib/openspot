# Survey canvas: design notes

Decisions behind the survey-canvas features (V-04 .. V-22). Behaviour only; no third-party
product code or assets were consulted.

## Persistence

All canvas data lives in `openspot-store/src/ext/survey.rs` (migration slot 3) and in
`openspot-model/src/canvas.rs` (`CanvasData`, keyed by zone/snapshot id so the core
`Zone`/`Snapshot` structs stay unchanged). Tables: `canvas_path`, `canvas_waypoint`,
`canvas_hidden`, `canvas_manual_ap`, `canvas_provenance`, `canvas_zone`, `canvas_crop_vertex`,
`canvas_drawing`, `canvas_shape`, `canvas_shape_vertex`. Entries equal to the defaults are
not written, and `CanvasData` equality ignores them.

## Measurements per point (V-09)

Several scans at one spot are averaged in the **linear power domain** (mW), then converted
back to dBm: the mean power is what a receiver integrates, while a dB-domain mean is dragged
down by deep fades (-50 and -60 dBm average to about -52.6 dBm, not -55). A network is kept
only if it appears in at least half of the scans (rounded up), and its average uses only the
scans that saw it. The data point radius presets (tiny .. extra, 1.5 m .. 25 m) set how far a
measurement is extrapolated; "automatic" keeps the earlier behaviour (6 % of the plan's long
side).

## Path mode (V-06)

Samples are placed at exact arc-length multiples of the interval along the polyline (corners do
not restart the spacing) plus the final waypoint. The run is scheduled on a walking pace
(default 0.8 m/s): sample `k` is due `k * interval / speed` seconds after the start, the clock
stops while paused, and a slow scan delays later samples instead of dropping them. Without a
scale the interval cannot be converted, so a fixed pixel spacing is used and a warning shown.

## PDF plans (V-08)

Page rendering uses `hayro`, a pure-Rust PDF rasterizer: no native library is bundled or
downloaded, so Windows and Linux builds are identical. It is pinned to `=0.5.0` because
`hayro` 0.6 and later (via `vello_cpu`, `hayro-jbig2`, `hayro-ccitt`) require Rust 1.92 and the
workspace targets 1.88; raise the pin when the workspace MSRV moves. Known renderer limits
(encrypted files, some blend modes) produce an error or an imperfect render, never a silently
blank plan. `pdfium-render` was not needed and is not used. Pages are rendered at 150 dpi with the
long edge capped at 4096 px.

## Drawing tools (V-07)

Shapes are stored as vectors and rendered with `tiny-skia` (lines, rectangles) and `ab_glyph`
(text) to the zone's map PNG after every committed edit. Text uses the bundled Lato Regular
font (SIL Open Font License 1.1, `crates/openspot-app/assets/fonts/OFL.txt`).
