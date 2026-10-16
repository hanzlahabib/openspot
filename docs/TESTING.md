# Testing

`cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must pass
before every commit. Pure logic (interpolation, contours, sorting, autosave decisions, store
round trips) is tested without a display; egui drawing code is not unit-tested.

## Golden images (Q-03)

`crates/openspot-export/tests/golden.rs` renders a tiny fixed scene (a 96x64 plan with five
measurement points) through the same export path users get, decodes the PNG, and compares it
pixel for pixel with a reference in `crates/openspot-export/tests/golden/`.

Cases cover the default gradient, a single AP with a preset and a custom value range, the
troubleshooting classes and the AP-count map. Two meta-tests check that the comparison itself
notices a one-pixel change and that presets and ranges really change the output, so the
goldens cannot pass vacuously.

Comparison is on decoded RGBA pixels, not file bytes: PNG encoder or compression changes do
not matter, any color change does.

### When a golden test fails

1. The failing run writes the produced image to `target/tmp/<name>.actual.png` and prints how
   many pixels differ and the first one.
2. Look at it. If the change is a bug, fix the code.
3. If the change is intended (new gradient colors, different interpolation), regenerate:

   ```sh
   UPDATE_GOLDEN=1 cargo test -p openspot-export --test golden
   ```

   Then review the changed PNGs in the diff and commit them together with the code change.

Without `UPDATE_GOLDEN`, a missing golden is a test failure, never silently created.

The scene uses only deterministic arithmetic (no randomness, clocks or threads), so results
are identical across machines. If a platform ever differs by one rounding step, fix the
source of the difference instead of loosening the comparison.

## Background jobs

`openspot-heatmap` tests cover the job runner: result equality with the synchronous path,
prompt cancellation of a multi-second rasterization, progress, panic reporting, and thread
joining on drop (an `Arc` witness proves the worker finished before `Drop` returns).
