#!/usr/bin/env bash
# Builds the Windows portable zip and SHA256SUMS from Linux or WSL, for binaries cross-compiled with
#   cargo build --release --target x86_64-pc-windows-gnu -p openspot-cli -p openspot-app
# Same output as build-portable.ps1 (same names, same layout). Needs `zip` and `sha256sum`.
#
# Usage: build-portable.sh [BIN_DIR] [OUT_DIR]
#   BIN_DIR  default: target/x86_64-pc-windows-gnu/release
#   OUT_DIR  default: dist
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bin_dir="${1:-$repo/target/x86_64-pc-windows-gnu/release}"
out_dir="${2:-dist}"

# The version comes from [workspace.package] in Cargo.toml: the single source of truth.
version="$(awk '/^\[workspace\.package\]/{f=1; next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/, "", $3); print $3; exit}' "$repo/Cargo.toml")"
[ -n "$version" ] || { echo "no version under [workspace.package] in Cargo.toml" >&2; exit 1; }
name="openspot-v${version}-windows-x86_64-portable"

for p in openspot.exe openspot-gui.exe; do
  [ -f "$bin_dir/$p" ] || { echo "missing $p in $bin_dir; build the release binaries first" >&2; exit 1; }
done

mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/$name"
cp "$bin_dir/openspot.exe" "$bin_dir/openspot-gui.exe" "$stage/$name/"
for f in README.md CHANGELOG.md LICENSE-APACHE LICENSE-MIT; do cp "$repo/$f" "$stage/$name/"; done

rm -f "$out_dir/$name.zip"
# -X drops extra file attributes and -r recurses; the staging folder is the single top-level entry.
(cd "$stage" && zip -q -X -r "$out_dir/$name.zip" "$name")

# `sha256sum` format ("<hex>  <name>"); sorted by name like the PowerShell version.
(cd "$out_dir" && find . -maxdepth 1 -type f ! -name SHA256SUMS -printf '%f\n' | LC_ALL=C sort | xargs sha256sum > SHA256SUMS)
echo "wrote $out_dir/$name.zip"
