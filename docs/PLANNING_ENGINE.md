# Planning engine

Pure-Rust, deterministic prediction of Wi-Fi signal for floor plans without measurements.
No UI here: the Planning UI is built on this API. Data lives in `openspot-model`
(`planning` module, `Zone::planning`), persistence in `openspot-store`
(`ext/planning.rs`, migration slot 4), computation in `openspot-predict`.

Roadmap coverage: P-02, P-03 (data model), P-04 (format + parser, see decisions), P-05,
P-06, P-07, P-08 (optional mode), P-09 (coverage placement), P-10 (engine side).
P-01 (drawing) and the P-03 placement UI belong to the UI wave.

## Conventions

* Meters everywhere in planning data. Plan frame: x right, y down (like the map);
  convert map pixels with `Scale` (`pixels_per_meter`).
* A zone is a level. `PlanningData.level` carries `elevation_m` (floor height above the
  building reference), `floor_height_m`, an alignment `offset` added to the level's
  coordinates, and the `slab` (the floor **below** the level). `stack_elevations(zones, base)`
  fills elevations from floor heights for zones ordered bottom to top.
* Azimuth angles are `atan2(dy, dx)` in plan coordinates (clockwise on screen). Tilt is
  downward-positive. Elevation angles are positive up.
* `Radio.channel` is the **center** channel number; center frequency follows IEEE numbering
  (`Band::channel_center_mhz`).
* Materials resolve by id: the zone's custom `materials` first, then the preset catalog.
  Custom materials are per zone (there is no project-level slot in the model); the UI should
  copy a material into each zone that uses it.

## Materials (P-02)

`MaterialCatalog::builtin()` loads `crates/openspot-model/data/materials.json`
(`version`, `provenance`, materials). Each material has `absorption_db` (dB per crossing)
and `reflection` (magnitude 0..1) for 2.4/5/6 GHz, `category` (wall, door, window, shelf,
slab), `reference_thickness_m`, `source` and the ITU-R P.2040 coefficients it was derived from.

Derivation (`slab_transmission_reflection`): complex relative permittivity from P.2040-3
Table 3 (`eps' = a f^b`, `sigma = c f^d`, `eps'' = 17.98 sigma / f`), then the standard
single homogeneous slab at normal incidence including internal multiple reflections.
Evaluated at 2.45 / 5.5 / 6.5 GHz. `Material::from_p2040` does this for user-supplied
coefficients and thickness; `Material::custom` takes dB values directly. A unit test
recomputes every catalog value from its coefficients, so file and code cannot drift.

Bump `version` whenever catalog values change (the UI can show a "presets updated" notice).

**UNVERIFIED**
* The Table 3 coefficients were read from the MathWorks `buildingMaterialPermittivity`
  reference page, which reproduces P.2040-3 Table 3; the ITU PDF itself was not opened.
  Re-check against the Recommendation before relying on them.
* Reference thicknesses (20 cm concrete, 10 cm brick, 2.5 cm plasterboard, 4 cm door,
  6 mm glass, 4 cm chipboard shelf) are typical-construction assumptions, not measurements.
  Real walls have studs, reinforcement, cavities and oblique incidence; measured losses
  differ. Calibrate (P-07) or define a custom material.
* Metal, concrete blocks, plaster-and-lath and other presets are not shipped because no
  public source was used for them.

## Propagation (P-05, P-06)

```
PL = FSPL(1 m, f) + 10 n log10(max(d, 1 m)) + sum(wall losses) + sum(slab losses) + offset
RSSI = Ptx + G(az, el) - PL
```
Free-space term `20 log10(4 pi d f / c)` evaluated at the reference distance; walls counted
explicitly along the straight 3-D TX->RX segment (multi-wall model; ITU-R P.1238 distance
power loss + floor penetration; COST 231 multi-wall). `n` defaults to 2.5 (`DEFAULT_EXPONENT`),
a prior only.

* `Scene::build(&[&PlanningData], &MaterialCatalog)` resolves materials and applies alignment
  offsets. `Scene::transmitters(..)` lists enabled radios. Both take levels bottom to top.
* `Scene::path_loss(model, tx, rx, freq_mhz, band) -> PathLoss` itemises distance, wall, floor
  loss and crossing counts. `rssi_dbm(..)` adds power and antenna gain.
* Vertical handling: the 3-D segment is cut at each slab it crosses. Each level's walls are
  tested only against the part of the path inside that level. Each slab adds its loss unless the
  crossing point lies in a cutout of the level above the slab (loss 0 there).
* Walls are full height. Openings (door/window) replace the wall material over their span.
* Crossing rules (`geom::wall_crossing`): a path endpoint lying on a wall does not cross it
  (reciprocal, and an AP mounted on a wall is not penalised); touching a wall's start counts,
  touching its end does not, so connected wall polylines are crossed once at shared vertices;
  running along a wall counts it once.
* Antenna gain: omni constant; directional uses the 3GPP TR 36.814 A.2.1.1 parabolic
  pattern (`-min(12 (x/bw)^2, Am)` per plane, combined and capped at the front-to-back ratio);
  table patterns add the interpolated azimuth and elevation cuts (separable approximation).

## Grids and combined views (P-05, P-10)

```rust
let scene = Scene::build(&levels, &catalog)?;
let tx = Scene::transmitters(&levels)?;
let req = GridRequest { level: 0, width_m, height_m, cell_m: 0.5, rx_height_m: 1.0,
                        pixels_per_meter: scale.pixels_per_meter(), mode: Mode::Direct };
let token = CancelToken::new();
let control = Control { cancel: Some(&token), progress: Some(&|p| /* p.done / p.total rows */ ()) };
let set = PredictionSet::compute(&scene, &model, &tx, &req, &control)?;
let grid = set.strongest(Some(Band::Ghz5));   // openspot_heatmap::Grid, feed the colorizer
```
* `rssi_grid` gives one `openspot_heatmap::Grid` per radio (`cell_px = cell_m * pixels_per_meter`,
  every cell filled, values in dBm, sampled at cell centers). Predicted APs on other levels
  contribute through slab losses.
* `PredictionSet` keeps all per-radio grids (`radios`, with ap id, band, channel center,
  width) and derives `strongest`, `ap_count(threshold)`, `snr(noise)`, `sir(heard_dbm)`
  (same co-channel model as the survey SIR map: bandwidth-overlap weighted, mW sum, capped
  at `MAX_SIR_DB`) and `serving_radio` (category grid; value = index into `radios`).
  Per-AP export is `radios[i].grid`.
* The `parallel` feature (default) computes rows on rayon; output is bit-identical to the
  serial path (each cell is independent, rows are collected in order). Cancellation is checked
  per row and returns `PredictError::Cancelled`; progress reports finished rows (callbacks may
  come from worker threads). Grids above `MAX_CELLS` are rejected.
* `Mode::SingleBounce` (P-08) adds power-summed single-bounce wall reflections.

## Calibration (P-07)

`calibrate(&scene, &transmitters, &[Measurement]) -> Calibration { exponent, offset_db,
rmse_db, residuals_db, points }`. Known wall and floor losses, FSPL(1 m), transmit power and
antenna gain are subtracted; ordinary least squares then fits `loss = n * 10 log10(d) + offset`.
Apply with `Calibration::model()`. Refused: fewer than 3 points, non-finite readings or unknown
transmitter index, distances spanning less than a 2:1 range (`DegenerateGeometry`), and fits with
`n <= 0` or `n > 8` (`ImplausibleExponent`). Calibration is direct-path only. With three points
the fit has one degree of freedom, so the RMSE is optimistic; prefer many points spread over
distances and behind walls.

## Decision records

**P-04 antenna patterns.** Parametric directional patterns and a documented JSON table format
are implemented (`AntennaTable::from_json`, validated: finite, strictly increasing angles,
>= 2 points per cut, azimuth within one turn, elevation within +-90). Gains are dB relative to
peak; peak comes from `Radio.antenna_gain_dbi`. Azimuth interpolates linearly in dB and wraps;
elevation clamps at the ends. Example:

```json
{"name":"patch","azimuth":[[-180,-25],[-90,-12],[0,0],[90,-12],[180,-25]],
 "elevation":[[-90,-25],[-45,-8],[0,0],[45,-8],[90,-25]]}
```
Vendor formats (e.g. MSI Planet, EDX/NSMA) are **not** implemented: no public specification
was consulted, and guessing a format from sample files would not be clean-room. If wanted,
implement from the published format specification and convert into `AntennaTable`.
Limits: a separable two-cut approximation of the true 3-D pattern; the "omni" pattern is
isotropic in elevation (real omni antennas null overhead, which matters for floor-to-floor
paths).

**P-08 ray tracing.** Implemented as an optional single-bounce image-method mode only
(`reflect.rs`): same-level walls, power summation, no fading, no double bounces, no
diffraction, slabs and openings do not reflect. Full ray tracing is deliberately not
attempted: without diffraction and multi-bounce it would be more expensive but not more
trustworthy than the calibrated multi-wall model.

**P-09 placement.** `placement::suggest_placements`: greedy maximum coverage over a candidate
lattice (`candidate_lattice`) for a radio template and a threshold, given existing APs of the
same band. No channel planning and no interference term: those need an objective the product
owner should choose (e.g. maximise min SIR).

## Not verified here

No radio hardware and no real survey data were available, so the model's absolute accuracy
against real buildings is **unverified**; tests prove the implementation matches its formulas
(analytic FSPL, exact wall sums, closed-form SIR, slab formula limits, parameter recovery from
synthetic data), not that the formulas match a given building. Calibrate against measurements
before trusting numbers.
