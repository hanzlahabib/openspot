#!/usr/bin/env bash
# EXPERIMENTAL SKETCH, never run (there is no macOS machine in the development environment):
# wraps a release build of openspot-gui into OpenSpot.app and ad-hoc signs it.
#
# Usage: build-app-bundle.sh [path/to/openspot-gui]   (default target/release/openspot-gui)
# A bundle without an icon (.icns) shows the generic application icon.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
binary="${1:-$repo/target/release/openspot-gui}"
version="$(awk '/^\[workspace\.package\]/{f=1; next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/, "", $3); print $3; exit}' "$repo/Cargo.toml")"
app="$repo/dist/OpenSpot.app"

[ -f "$binary" ] || { echo "missing $binary; build it on a Mac first" >&2; exit 1; }
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/openspot-gui"
sed "s/@VERSION@/$version/g" "$repo/packaging/macos/Info.plist.template" > "$app/Contents/Info.plist"

# Ad-hoc signature ("-"): enough to run locally, NOT enough for distribution. Gatekeeper rejects
# an ad-hoc-signed download unless the user overrides it. See README.md in this folder.
codesign --force --deep --sign - "$app"
echo "wrote $app"
