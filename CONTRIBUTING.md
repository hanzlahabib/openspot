# Contributing to OpenSpot

Thanks for helping. OpenSpot is built from public specifications, and every feature is tracked
in [docs/ROADMAP.md](docs/ROADMAP.md) with a stable ID.

## Ground rules
1. **Read [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md).** Do not contribute code, strings, icons or data
   taken from proprietary products. Describe behaviour and write your own implementation from specs.
2. **One checkpoint per commit.** Each commit builds and passes
   `cargo test --workspace` and `cargo clippy --workspace --all-targets`.
3. **Conventional Commits** with the roadmap ID: `feat(heatmap): signal-to-interference map [H-05]`.
4. **Tests with every feature.** Pure logic gets unit tests; visual output gets a fixture/golden test.
   Features that read radios also need a note on how you checked them on real hardware.
5. Keep files under about 600 lines and comments explaining *why*, not *what*.
6. Never commit `analysis/`, captured private data, or real SSID/BSSID lists of other people.

## Workflow
Branch from `develop`, open a PR to `develop`. Maintainers merge `develop` to `main` for releases.

## Dev setup
```sh
cargo test --workspace
cargo run -p openspot-app -- --fake        # GUI with demo data
cargo build --release --target x86_64-pc-windows-gnu -p openspot-app   # Windows build from Linux/WSL
```

By contributing you agree your work is licensed under MIT OR Apache-2.0.
