# Planning mode (UI)

The Planning app mode (Inspector | Survey | **Planning**) draws a floor plan's walls and
virtual access points and shows the engine's coverage prediction (`docs/PLANNING_ENGINE.md`).
Code: `crates/openspot-app/src/planning/` (logic and state at the top level, egui layer in `ui/`).

Roadmap coverage: P-01 (drawing), P-03 (AP placement and settings), P-06 (level selector,
stairwells, slab), P-07 (calibration UI), P-09 (suggestions UI), P-10 (prediction views).

## Shared with Survey

A zone's floor-plan image, **scale** and snapshots are the same ones Survey uses; planning
geometry lives in `Zone::planning` in meters and is saved in the same project file. Planning
needs a scale (a plan of unknown size has no meters): without one the canvas offers "Set scale"
(treat the image width as N meters) and prediction refuses with a message. A blank plan of a
known size (`Create`) has its scale already.

Shared display code lives in `heat_ui.rs` (gradient presets, smoothing, opacity, legend bar, hover
text) and `map_view.rs` (plan texture decoding, wheel zoom); Survey uses the same functions.

## Drawing (P-01)

Tools: Select, Wall, Rack, Door, Window, Stairwell, Access point, Delete.

| Action | How |
|---|---|
| Wall / rack | drag, or click points to chain segments; Esc or right-click ends |
| Door / window | click a wall; sized from the side panel, kept on the wall, refused if it overlaps another opening |
| Stairwell | click the corners of the opening in the floor slab; click the first corner or Enter closes |
| Move | Select tool: drag an element; drag a wall end to reshape; drag an opening along its wall |
| Delete | Delete tool, or select and press Delete / Backspace |
| Undo / redo | Ctrl/Cmd+Z, Ctrl/Cmd+Y (or +Shift+Z); every edit is a small reversible op, not a snapshot |
| Pan / zoom | middle-drag or Space+drag (or drag empty space with Select); wheel |

Snapping, in priority order: existing wall ends and stairwell corners, then 15 degree angle
steps (with the length rounded to the grid), then the grid (0.1 / 0.25 / 0.5 / 1 m). Each can be
turned off. Pick distance is a fixed 8 screen pixels, converted to meters at the current zoom.

The tool logic is a pure state machine (`tools.rs`) fed `Hover/Press/Drag/Release` events in plan
meters; `geometry.rs` has snapping and hit testing; `edit.rs` the edits and history.

### Materials

Wall, rack, door and window materials come from the shipped catalog (read-only) or the zone's
custom materials (Materials... window: name, category, loss per crossing and reflection for
2.4 / 5 / 6 GHz). Custom ids are `custom-<name>`, never equal to a preset id, so a catalog update
cannot change a user's wall. A custom material in use cannot be deleted. Custom materials belong
to one level; "Copy to other floors" copies one into the other floors of the building.

## Access points (P-03)

Place with the Access point tool; select one to edit name, mounting height and per radio: on/off,
width, channel (only valid centers for the band and width are offered), transmit power, antenna
gain and pattern (omnidirectional, or directional with direction, horizontal and vertical
beamwidth, downtilt and front-to-back). Edits to a field commit when it is released, so one
adjustment is one undo step.

### Model provenance policy

OpenSpot ships **no vendor or product catalog**. Datasheet values cannot be sourced and kept
accurate here, and a wrong antenna pattern or power silently yields a wrong map. The built-in
models are named "Generic ..." and are round-number typical classes chosen by this project
(for example 17 dBm / 3 dBi at 2.4 GHz and 20 dBm / 4 dBi at 5 GHz for the indoor class); they
are not the specification of any product. Users who know their hardware set the values, then
"Save as model": saved models are kept in the preferences file (`planning.ap_models`) and appear
next to the generic ones. Do not add product names or figures copied from datasheets or other
tools to the built-in list.

## Prediction and views (P-10)

The engine runs on a background thread (the shared `openspot_heatmap::Job`: progress, cancel,
joined on drop). It restarts only when an input changes: edits (a generation counter), the
propagation model, detail, receiver height, wall-reflection mode, or anything Survey changes
that matters (scale, plan image, building structure). "Update automatically" can be turned off
(then "Predict now"), and Stop prevents an automatic restart for those same inputs.

Views: strongest signal, one radio (with previous/next quick switch), number of access points
(radios at -80 dBm or better), interference (co-channel signal-to-interference, radios below
-90 dBm ignored), serving access point. Band filter, gradient preset, smoothing (off by default:
predictions are exact per cell), opacity, legend and hover value as in Survey. **Signal-to-noise
is not offered**: OpenSpot has no noise-floor source on any platform (`docs/NOISE_FLOOR.md`);
the option is shown disabled with that explanation. Export PNG writes the plan with the shown
view and the access points through `openspot_export::render_png_raster`.

Detail sets the grid (about 100 / 200 / 400 cells along the longer side, never finer than 10 cm).

## Levels (P-06)

A zone grouped into a building in the Survey explorer is a floor. Predicting a floor includes
all floors of its building, bottom to top: elevations come from the floor heights of the
structure, the stored level offset plus the floor alignment translation is the engine's level
offset, and the slab under a level (material chosen in the side panel) is charged to signals
crossing it except through that level's stairwell cutouts. Floor rotation in the alignment is
not applied (a warning is shown). The level selector in the toolbar switches floors; the floor
below can be shown faintly to line up stairwells. A zone outside any building is a single level.

## Calibration (P-07)

"Calibrate to survey..." picks a snapshot of the shown zone and the networks that are the planned
system (none ticked: all heard). Each survey point contributes its strongest chosen network,
compared with the planned radio of the same band that the model in use predicts to be loudest
there. At least 3 comparable points spread over at least a 2:1 range of distances are required
(engine rule). The window shows exponent, offset and RMSE before and after; Apply makes the
prediction use the fit (and recompute), Reset returns to the default model. With few points the
RMSE is optimistic; the window says so.

The calibrated model is **not saved in the project** (the data model has no slot for it); it
lasts for the session.

## Suggestions (P-09)

"Suggest AP positions..." asks the engine for N new access points that bring the most of the
plan to a chosen signal level in a chosen band, on top of those already placed, trying a lattice
of positions (kept clear of walls, coarsened so it never exceeds the engine's candidate limit).
Proposals show as numbered rings with their gain; each can be added as a real access point (the
model chosen for new access points) or all at once, as undoable edits. Coverage only: no channel
planning, no interference term.

## Verification status

Everything below the egui layer is unit-tested (tool state machines, snapping, hit tests,
history, material rules, AP value rules, channel tables, level stacking, calibration inputs,
view rendering, background job equals synchronous engine result, preference round trips) and
`planning/e2e.rs` builds a two-room scene through the same state API the canvas uses and checks
predicted levels against the closed-form model (FSPL + 10 n log10 d + wall + slab).

**UNVERIFIED** (no display was available): all drawing and mouse/keyboard wiring in
`planning/ui/` (layout, colors, hit areas, hover, panning, window behaviour), the PNG export's
appearance in an image viewer, and behaviour at high DPI. Model accuracy against real buildings
is unverified, as in the engine docs: calibrate before trusting numbers.
