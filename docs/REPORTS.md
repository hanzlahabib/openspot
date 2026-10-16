# Reports and interchange (`openspot-report`)

Covers PDF reports (R-03, R-04), branding (R-05), per-AP heatmap export (R-06) and
project archives (R-07).

## PDF library decision

| Option | Verdict |
|---|---|
| `printpdf` 0.12 | Rejected. Default features pull a full HTML/CSS layout engine (`azul-*`, `rust-fontconfig`) for what is a fixed set of pages; the API changed incompatibly in 0.5, 0.7, 0.8 and 0.12; font handling depends on system font discovery unless carefully avoided. |
| `genpdf` 0.2 | Rejected. Unmaintained since 2021, sits on an old `printpdf`/`rusttype`, and has no way to attach a `ToUnicode` map or control determinism. |
| `krilla` 0.8 | Rejected for now. Would be a good fit, but needs Rust 1.92; the workspace pins 1.88. Revisit when the MSRV moves. |
| **`pdf-writer` + `ttf-parser`** | **Chosen.** `pdf-writer` is a thin, dependency-light, typed writer (the PDF backend of Typst). All layout is ours (a few hundred lines in `src/report/builder.rs`), which keeps the output byte-for-byte reproducible and lets every embedded font carry a `ToUnicode` map so text stays selectable and searchable. |

Tests parse the output with `lopdf` (a different implementation than the writer), so the
check does not share bugs with the generator.

## Fonts, non-ASCII text and fallback

Reports embed **Noto Sans** Regular and Bold (SIL OFL 1.1, `crates/openspot-report/fonts/OFL.txt`),
subset with `pyftsubset` to Basic Latin, Latin-1/Extended-A/B, Greek, Cyrillic and general
punctuation (about 82 KB each; regenerate from the upstream hinted TTFs with
`pyftsubset NotoSans-Regular.ttf --unicodes=U+0020-007E,U+00A0-024F,U+0370-03FF,U+0400-052F,U+2010-2027,U+2030-203A,U+20AC,U+2190-2193,U+2212,U+2264-2265,U+FFFD --no-hinting --layout-features= --name-IDs='*' --glyph-names`). Nothing is read from the system, so a PDF looks the same
on every machine.

Text is normalised before drawing (`FontFace::sanitize`):

1. NFC composition, so a decomposed `e` + U+0301 becomes one glyph (the font is not shaped, so
   combining marks would otherwise float).
2. Whitespace (tabs, newlines, NBSP) becomes a plain space; zero-width and bidi control
   characters are dropped.
3. Any character without a glyph (CJK, Arabic, Hebrew, emoji, Indic, ...) is drawn as
   **U+FFFD** (one per character). Layout never breaks; the BSSID column identifies the AP.

Not supported: complex-script shaping and right-to-left reordering. A CJK-capable font is too
large to bundle (several MB per weight); a future opt-in could embed a user-supplied font.

`≤`/`≥` are absent from Noto Sans, so the reports print `<=`/`>=`.

## Report structure

1. **Cover**: logo, company, title/subtitle (custom or default), project, client, location,
   contact, survey dates (first to last measurement day), report date.
2. **Summary**: counts, optional notes, zone/snapshot table, network table (SSID or alias,
   BSSID, channel, band, width, security, vendor via `MacAddress::vendor()`, max RSSI).
3. **One page per selected zone x snapshot x visualization**: heatmap on the plan, legend
   (gradient bar or labelled swatches), AP table with marker numbers. Long tables continue
   on following pages with a repeated header.

Heatmap numbering is `#zone.snapshot.visualization`, 1-based; the visualization number is
fixed (`Visualization::number`), so `#1.1.3` is always the SIR map.

The raster comes from `openspot_export::render_png` (the same renderer as the PNG export).
Measurement points, the walking path and AP markers are drawn as vector shapes on top, so
they stay sharp and are toggled without re-rendering. AP markers use
`estimate_ap_position` and need at least three readings.

**Quick** = every zone and snapshot, signal level only, default view. **Advanced** honours
zone/snapshot selection, any set of visualizations, view options (plan, path, points, AP
markers, gradient, smoothing, opacity), titles/notes, which sections to include and the AP
row limit. A Quick report ignores selections but keeps the user's titles.

Edge cases that still produce a valid PDF: no visualizations, no zones, all sections off
(one "Empty report" page), zones without a plan (placeholder), snapshots without points.

## Reproducibility

`ReportOptions::generated_at` (Unix seconds) is the report date *and* the PDF creation time;
the file id is a hash of the content. Same inputs and same `generated_at` give identical bytes
(tested). Output size is dominated by heatmap images (RGB, Flate, max 1800 px on the long side).

## Verification status

Verified in this environment: structure, text (extracted through the `ToUnicode` maps with
`lopdf`, and once cross-checked with `pypdf`), embedded images and soft masks, metadata,
determinism, pagination, cancellation, option combinations.

**UNVERIFIED**: how pages look. There is no display and no PDF renderer in the development
environment, so glyph rendering in real viewers (Acrobat, Preview, Chrome, Edge), visual
alignment, colours and the overlay positions on the plan were never seen by a human. Open a
sample (`OPENSPOT_REPORT_SAMPLE=/tmp/s.pdf cargo test -p openspot-report -- --ignored dump_sample`)
before relying on the layout.

## Per-AP heatmap export (R-06)

`export_per_ap_heatmaps` writes one PNG per access point and snapshot into a folder, each
showing the signal of that AP alone (`Metric::Signal([bssid])`; places where it was not heard
sit at the -100 dBm floor). Zone/snapshot selection and view options come from
`ReportOptions`. Zones without a floor plan are skipped and listed in the result.

File names: `<zone#>.<snapshot#> <zone> - <snapshot> - <alias or SSID> (<BSSID with dashes>).png`.
Each text part goes through `sanitize_file_name`: forbidden characters and controls become `_`,
whitespace collapses, trailing dots/spaces are trimmed, Windows device names (`CON`, `NUL`,
`COM1`...) are prefixed with `_`, and parts are cut to 60 bytes on a character boundary, so
the whole name stays under 255 bytes. The numeric prefix and the BSSID keep names unique.
Existing files with the same name are overwritten.

## Project archives (R-07)

`.ospotz` is a zip with exactly two entries: `manifest.json` (format version, app version,
creation time, and for each member its name, size and SHA-256) and `project.ospot` (the
SQLite project). It exists for sharing a project as one file; the app keeps working on the
plain `.ospot`.

Export serialises the in-memory project through the normal store (so unsaved edits are
included), packs it and renames the finished file into place. Import is strict and typed
(`ArchiveError`), in this order:

1. entry count cap; every entry name checked for zip-slip (`..`, `.`, empty components,
   absolute paths, backslashes, drive letters, NUL) and for being one of the two known names;
2. manifest: size-capped (1 MiB), valid JSON, `format_version` 1..=current, member names safe
   and known;
3. database streamed to a `.partial` file under a size cap (2 GiB, enforced on the byte
   stream, not on the declared size), with zip CRC, manifest length, SHA-256 and the SQLite
   header all checked;
4. the partial file is opened with the store and loaded before it replaces the destination.

On any failure the destination is untouched and the partial file is removed. Entries are never
extracted by name, so a hostile name cannot choose a path even if a check were missed.

Limits of the guarantee: checksums catch corruption and casual edits, not an attacker who
rewrites the manifest too (there is no signature). Duplicate entry names are collapsed by the
zip reader; the surviving entry must still match the manifest, so nothing unverified is
imported.

## App integration

Survey toolbar: **Export report…** opens the wizard (`survey/ui/report.rs`): Main (Quick or
Advanced, output file, open afterwards, per-AP heatmaps and folder), Zones & snapshots,
Visualizations, View options, Titles & branding. Quick shows only Main and Titles. Generation
runs on a background thread (`survey/ui/report_job.rs`) with a progress bar and **Cancel**;
nothing is written under the final name unless the PDF completed (`.partial` + rename).
**Archive ▾** exports the project as `.ospotz` or imports one (verify, write a `.ospot`, open it).

Branding (company, logo path, client, location, contact) lives in `Prefs.report` and survives
"New project". Not verified here (no display): how the window looks and behaves, the file
dialogs, and opening the finished PDF with the system viewer (`explorer` / `open` /
`xdg-open`).
