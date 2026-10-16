# Fuzzing the IE parser (Q-02)

`parse_ies` feeds arbitrary bytes to `openspot_ie::parse` and the element iterator.

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run parse_ies -- -max_total_time=60
```

Seed from real captures by copying `raw_ies` hex dumps (`openspot scan --json`) into
`fuzz/corpus/parse_ies/` as binary files (prefix each with one context byte).
Crashes are written to `fuzz/artifacts/`; turn each into a regression test in
`crates/openspot-ie/src/tests.rs`.

The property tests in `crates/openspot-ie/src/proptests.rs` run on stable in `cargo test`
and cover the same no-panic guarantee with shorter inputs.

## Verification status

`cargo-fuzz` itself could not be installed in the authoring environment (its dependency
`cargo-platform` needs a newer rustc than the pinned 1.88 workflow). The target was
instead built by hand with the same instrumentation flags `cargo-fuzz` uses (nightly
`rustc` via `RUSTC`, `--cfg fuzzing`, `-Cpasses=sancov-module`,
`-Cllvm-args=-sanitizer-coverage-level=4` with inline-8bit-counters, pc-table and
trace-compares, `--target x86_64-unknown-linux-gnu` so build scripts stay uninstrumented)
and run for 45 s: 4.87 M executions, no crash. The `cargo +nightly fuzz run` command
above is the supported path and is untested here.
