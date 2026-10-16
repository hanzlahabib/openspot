<!-- Branch from `develop` and open the pull request against `develop`. See CONTRIBUTING.md. -->

## What and why

<!-- One or two sentences. Link the issue if there is one. -->

## Roadmap IDs

<!-- From docs/ROADMAP.md, e.g. H-05, V-13. "none" is fine for a fix or a chore. -->

## Checks

- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] New logic has unit tests; visual output has a golden or fixture test
- [ ] Each commit is one checkpoint and builds on its own; commit messages follow Conventional Commits with the roadmap ID
- [ ] User-visible changes are described in `docs/` (and `CHANGELOG.md` under Unreleased)
- [ ] Files stay under about 600 lines; comments say why, not what

## Verification

<!-- What you ran and saw. For anything that reads a radio, a network or a real file format: the hardware or
     tool you checked it on and what it showed. If you could not verify something, say so here and mark it
     UNVERIFIED in the docs rather than leaving it implied. -->

## Clean-room statement

- [ ] I have read `docs/CLEAN_ROOM.md`. This change contains no code, text, icons, data or catalogs taken from
      proprietary products, and describes behaviour in my own words.
- [ ] No captured private data, no real SSID/BSSID lists of other people, nothing from `analysis/`.

## Licence

By opening this pull request I agree my work is licensed under MIT OR Apache-2.0.
