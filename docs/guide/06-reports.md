# 6. Reports and archives

All of this lives in the Survey toolbar. For design decisions and internals see
[REPORTS.md](../REPORTS.md).

## PDF report

**Export report...** opens a wizard.

**Main**

- **Report type**
  - *Quick*: every zone and snapshot, with the signal level map. One click to a usable report.
  - *Advanced*: you choose zones and snapshots, the visualizations and options.
- **File**: where to save the PDF (the name defaults to the project's name).
- **Open the report after export**.
- **Also export one heatmap image per access point**, into a folder you choose (default: next to the report).

**Zones and snapshots** (Advanced): tick what to include. One page is added per zone, snapshot and
visualization that has a floor plan.

**Visualizations** (Advanced): any combination of the maps in [Heatmaps explained](04-heatmaps.md).

**View options** (Advanced): show floor plan, path, points and AP positions, the colour scheme and
smoothing, and which parts to include: cover page, summary tables, the access point table on each heatmap page.

**Titles and branding**: title and subtitle, company, client, location, contact, logo (PNG or JPEG) and free notes for the summary
page. Company, client, location, contact and logo are **remembered** for next time and survive "New project";
the title, subtitle and notes are per report. Quick reports keep your titles.

Generation runs in the background with a progress bar and **Cancel export**. A report is written under
a temporary name and renamed on completion, so a cancelled or failed run never leaves a broken PDF
under the real name.

### What a report contains

1. A **cover**: logo, company, title, project, client, location, contact, the survey dates (first to last
   measurement day) and the report date.
2. A **summary**: counts, your notes, a zone and snapshot table, and a table of networks (name or alias, BSSID, channel,
   band, width, security, vendor, strongest signal).
3. One page per zone, snapshot and visualization: the heatmap on the plan, a legend, and a table of the access points
   with the numbers that appear on the plan's markers. Long tables continue on the following pages.

Heatmap pages are numbered `#zone.snapshot.visualization`; the visualization number is fixed, so the
same map keeps its number in every report. Measurement points, the path and AP markers are drawn as
vector shapes over the heatmap image, so they stay sharp when you zoom the PDF.

### Fonts and languages

Reports embed their own font (Noto Sans, SIL Open Font License), so they look the same on every
computer. It covers Latin, Greek and Cyrillic. Characters outside those, such as Chinese, Arabic,
Hebrew, emoji or Indic scripts, are printed as a replacement character, so a name in those scripts will be
unreadable in a report (the BSSID column still identifies the access point). Right-to-left text is not
reordered.

### Not verified

The PDF structure, text and images are checked by automated tests. **How the pages look** (glyphs,
alignment, colours, marker positions) has not been checked by a person in a PDF viewer. Look at a report before sending it to a client.

## Per-AP heatmap images

With the option above, or from the report wizard, OpenSpot writes one PNG per access point and snapshot,
each showing the signal of that AP alone. File names look like
`1.1 Ground floor - Snapshot 1 - Hallway AP (AA-BB-CC-DD-EE-FF).png`. Characters Windows does not allow in names are
replaced, long names are shortened, and an existing file with the same name is overwritten.

## Other exports

| Export | Content |
|---|---|
| **Export CSV...** | Every reading of the current snapshot, for spreadsheets |
| **Export PNG...** | The plan with the heatmap and points as shown |
| **Export SVG...** | Contour lines as vector graphics (turn **Contours** on) |

## Project archives (`.ospotz`)

**Archive > Export project archive...** packs the whole project (including unsaved edits) into one
`.ospotz` file for sharing; **Import project archive...** unpacks one into a new `.ospot` file and opens it.

An archive is a zip containing `manifest.json` and `project.ospot`. Import is strict: it checks the file names,
sizes, checksums and that the database opens, and on any failure it leaves the destination untouched.

**The checksum is not a signature.** It detects corruption and casual edits, not someone who rewrites the
manifest too. Do not treat an archive as trustworthy because it imports cleanly, and only open
archives from people you trust.
