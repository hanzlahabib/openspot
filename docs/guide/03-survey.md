# 3. Survey workflow

A survey is: a floor plan, a scale, and a set of points where you stood and scanned. OpenSpot
draws heatmaps from those points. This page walks through it in order.

Everything is stored in one project file (`.ospot`). The project contains **zones** (usually one per
floor or area, each with its own plan and scale) and each zone contains **snapshots** (one set of
measurements, for example "before moving the router" and "after"). Zones can be grouped into
**buildings** with floor labels.

## 0. Start page

Opening Survey shows a start page with **New project**, **Open...**, **Continue current** and a
table of recent projects (name, created, changed, last opened, zone count; click a column to sort,
click a name to open). Times are shown in UTC.

## 1. Get a floor plan

Choose one of three ways, in the toolbar:

- **Load floor plan...** PNG, JPEG or PDF. A PDF with several pages asks which page to use; only
  that one page is rendered.
- **Blank plan...** An empty grid of the width and height you enter. The scale is set for you.
  Good when you have no drawing and will walk a known area.
- **Draw plan...** Draw the plan inside OpenSpot (below). You enter the overall size first and the
  scale is set for you. This replaces the zone's current plan.

Plans bigger than 4096 pixels on the long side are shown downscaled on screen only; coordinates stay
in the original pixels.

### Drawing tools

After **Draw plan...** the **Draw** tool shows a bar: **Select**, **Line**, **Rectangle**, **Text**,
line thickness and font size presets, **Fill rectangles**, **Finish line**, **Apply style**,
**Edit text** and **Delete**.

- Line: click points, press **Enter** or **Finish line** to end it.
- Rectangle: click two opposite corners.
- Text: click where it goes, type in the dialog, **Place**.
- Select: click a shape, drag to move it. **Delete** removes it. **Esc** abandons the current action.

A drawn plan is kept in the project as shapes and rendered to the zone's image, so it stays editable.

## 2. Calibrate the scale

Heatmap distances, radius presets and path spacing all need to know how many pixels are a metre.
Until you set a scale the status bar says "scale not set" in yellow.

- **Calibrate scale**: click two points a known distance apart (a door-to-door span, a printed
  scale bar), then enter the real distance in the dialog.
- **Calibrate by rectangle**: click two opposite corners of a room or object whose real width and
  height you know, and enter both. If the proportions you clicked differ noticeably from the sizes
  you typed, a warning says so and the scale is an average of the two axes.

Use **Units** (m, cm, ft, in) to enter and display lengths in the unit you think in. The scale is
stored in metres.

## 3. Measure

Select the **Measure** tool (the default). Stand at a spot in the building, click that spot on
the plan, and OpenSpot scans and adds a point. While scanning the status bar shows "scanning...";
stay still. The point appears coloured by its strongest signal. Hover a point to see the networks
heard there, strongest first.

Practical advice: measure at the height and orientation your device will be used, and measure in
more places than you think you need, including both sides of every wall you care about. Heatmaps
are only as good as their points.

**Measurements per point** (menu **Sampling**) takes several scans at each click and combines them:
signal is averaged as power (milliwatts), not as dB, because the average power is what a receiver
sees and a dB average is dragged down by deep fades. A network is kept only if it was seen in at
least half of the scans.

**Data point radius** (also in **Sampling**) says how far a point's value is allowed to reach on
the map. Choose by environment: Tiny 1.5 m (cubicles), Small 3 m (apartments, walled offices),
Medium 6 m (open-plan offices, shops), Large 12 m (halls, warehouses), Extra 25 m (stadiums,
outdoors). **Automatic** extends by 6 % of the plan's long side. Presets need a scale.

**Undo** and **Redo** (Ctrl+Z, Ctrl+Y) cover measurement points, the scale and merges. They do not
cover zone and snapshot management or display settings.

### Path mode

Instead of clicking at every spot you can walk a route.

1. Choose the **Path** tool and click waypoints along the route you will walk.
2. Set **Sample every** (a distance) and **walking at** (a speed, default 0.8 m/s).
3. Press **Start** and begin walking. OpenSpot places a sample at the chosen spacing along the
   route and scans at the moment you should reach it, assuming a constant pace. A progress bar
   and "next in N s" show where it is; **Pause**, **Resume** and **Cancel** are available.

It assumes you walk as planned; if you stop or take a detour, the points sit in the wrong places.
A route is limited to 2000 samples. Without a scale, spacing falls back to 60 pixels and 3 seconds.
Use click-to-measure instead when accuracy of position matters more than speed.

## 4. Look at the maps

The metric picker, in the second toolbar row, chooses what the heatmap shows. All maps are explained
in [Heatmaps explained](04-heatmaps.md). Controls that apply to every map:

- **Colours** presets (Default, Red to green, Blue to red) and a custom **value range** under the
  legend (**Reset range** restores the default).
- **Smoothing**: Off, Normal, Strong, Extra strong.
- **Opacity** slider, **Heatmap**, **Points**, **AP positions** checkboxes and the **Show** menu
  (floor plan, survey path, sample indicators: simple or with signal, AP markers, AP titles, marker
  size).
- **Contours**: isolines instead of or on top of the colour fill, with a chosen number of lines.
  **Export SVG...** writes them as scalable vector graphics.
- **Troubleshooting**: shows regions as good, acceptable or critical against thresholds you can
  edit, for the signal, secondary signal and SIR maps.
- Hover anywhere on the map for the value there.
- The right panel lists **Networks**: tick the ones to include (**All**, **None**), set an alias,
  or click the target button to map **that network only**, with arrows to step through networks.
  Maps that depend on the selection are marked in [the next page](04-heatmaps.md).

Heatmaps are calculated on a background thread; a progress bar appears in the status bar for big
snapshots and the previous picture stays until the new one is ready.

### Crop and gray-out

The **Crop** tool limits where the heatmap is drawn and exported, as a rectangle or a polygon.
**Gray out unmeasured areas** tints everything farther than the data point radius from any
measurement, which makes visible where the map is guessing. **Remove crop** restores the full plan.

### AP positions

For every network ticked in the list with at least three readings, OpenSpot shows a diamond
at an **estimated** position: the centre of the strongest readings, weighted by power. The
estimate is a hint, not a survey of where the access point actually is. Drag a diamond to place it
by hand (it is then marked "manual"); **AP positions** menu resets one or all of them.

### Hidden networks

Networks with no SSID are kept. The **Hidden networks** menu lists them with their BSSIDs, best
signal and number of points; give each a label, and choose whether to include it in heatmaps and
exports.

## 5. Organise: zones, snapshots, explorer

- **+ zone**, **+ snapshot**, and the **Manage** menu: rename, duplicate, delete (with a warning
  that deleting cannot be undone).
- **Merge...** combines two or more snapshots of a zone into a **new** snapshot; the originals stay.
  Points closer than 0.5 m (6 pixels without a scale) and taken within the same hour count as
  duplicates. The dialog lists where each merged point came from.
- **Explorer** (toggle in the toolbar): a tree of project, buildings, zones and snapshots.
  Create buildings, move zones into them, order floors with **Floor up** and **Floor down**
  (right-click a zone).

## 6. Save, backup and recovery

- **Save** writes the `.ospot` file. If the file already exists, the previous contents are copied to
  `<name>.ospot.bak` first, so one step back is always there.
- OpenSpot **autosaves** a changed project to `<name>.ospot.autosave` every 2 minutes by default
  (change it under **Manage**, 0 turns it off). The main file is never overwritten by autosave.
  After a crash, opening the project offers **Recover** when the autosave is newer than the file.
- Autosave needs a file path: a project that has never been saved is not autosaved.

## 7. Export

**Export CSV...** (every reading), **Export PNG...** (plan, heatmap and points exactly as shown),
**Export SVG...** (contours), **Export report...** (PDF), and the **Archive** menu: see
[Reports and archives](06-reports.md).

Active scan (speed tests at each point): [Active scan](05-active-scan.md).
