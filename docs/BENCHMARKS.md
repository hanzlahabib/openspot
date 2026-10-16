# Benchmarks (Q-04)

Run with `cargo bench -p openspot-heatmap --bench interpolate` and
`cargo bench -p openspot-ie --bench parse`. `cargo bench --no-run --workspace` must keep
compiling. Benchmarks use `criterion`; inputs are deterministic (fixed xorshift seed).

## Baseline (2026-10-16)

Environment: WSL2 on Windows 11, rustc 1.88.0, release profile, single run,
`--warm-up-time 1 --measurement-time 5` for the heatmap group (10 samples). Numbers are a
reference point for regressions on this machine, not a cross-machine guarantee. CPU:
13th Gen Intel(R) Core(TM) i9-13900HX, 12 logical cores.

Heatmap, 10 000 scattered samples on a 2000 x 1500 px plan, 4 px cells (500 x 375 cells):

| Benchmark | Time (median of estimate) |
|---|---|
| `interpolate_10k_triangulated` (Delaunay + barycentric) | 41 ms |
| `interpolate_10k_with_edge_extension` (adds IDW over empty cells) | 44.5 ms |
| `smooth_strong` (box blur, radius 2) on the result grid | 9.4 ms |
| `rasterize_10k_signal` (snapshot sampling + interpolate + smooth, what the app calls) | 52.6 ms |
| `nearest_10k_category` (categorical metrics, 40 px reach) | **101 ms** (was 1.37 s before the spatial index, see below) |

IE parser, one Wi-Fi 7 style beacon (about 175 bytes, 12 elements):

| Benchmark | Time |
|---|---|
| `parse_wifi7_beacon` | 198 ns (about 850 MiB/s) |

## Spatial index for categorical maps (H-19 groundwork)

`heatmap::nearest` used to compare every cell with every sample (cells x samples, 1.37 s at
10 000 points). It now buckets the samples into a uniform grid whose squares are at least as
wide as the reach, so each cell inspects only the 3 x 3 buckets around it
(`crates/openspot-heatmap/src/nearest_index.rs`).

Re-run on the same machine with the same settings as the baseline above (load average about
11 from other builds, so treat the figure as approximate):

| Benchmark | Before | After |
|---|---|---|
| `nearest_10k_category` | 1.37 s | 96-108 ms (median estimate 101 ms), about 13.5 x faster |
| all other benchmarks | unchanged within noise | |

The result is identical to the brute-force scan, not just close: the same squared-distance
expression is used, and ties go to the sample that comes first in the input, as `min_by`
does. `grid::tests::spatial_index_gives_exactly_the_brute_force_result_on_random_inputs` compares
the two on 200 random inputs (free and lattice positions so many distances tie, duplicates,
samples outside the plan, radii from 0 to 1e6) bit for bit, and
`hostile_samples_and_radii_behave_like_the_scan` covers NaN and infinite samples and radii. The
brute-force reference lives in the test module so any future change to the index is checked
against it. Changing the bucket size, the tie rule or the bucket lookup makes these tests fail.

## Findings

* Continuous maps are comfortably interactive at 10 k points (about 50 ms).
* Categorical maps are now around 0.1 s at 10 k points, fast enough to recompute when the metric
  or smoothing changes, though still on the UI path; background rendering (H-19) already moves
  the work off the UI thread.
