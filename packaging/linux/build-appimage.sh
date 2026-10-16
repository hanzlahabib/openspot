#!/usr/bin/env bash
# EXPERIMENTAL SKETCH, never run: builds an AppImage from a release build.
#
# Needs: a release build (cargo build --release -p openspot-app -p openspot-cli), `appimagetool`
# on PATH (https://github.com/AppImage/appimagetool), and a 256x256 PNG icon. THERE IS NO ICON IN
# THE REPOSITORY, so this script stops until one is provided via ICON=path/to/openspot.png.
#
# Known gaps: the AppImage bundles only the two programs, not GTK or the Wayland/X11 libraries, so it
# relies on the host having them; Wi-Fi scanning needs CAP_NET_ADMIN (an AppImage cannot carry file
# capabilities, so users need `sudo setcap` on the extracted binary or must run as root); the Linux
# scanner itself is unverified on hardware.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bin_dir="${BIN_DIR:-$repo/target/release}"
icon="${ICON:-}"
version="$(awk '/^\[workspace\.package\]/{f=1; next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/, "", $3); print $3; exit}' "$repo/Cargo.toml")"

[ -n "$icon" ] && [ -f "$icon" ] || { echo "set ICON=<256x256 png>; the repository ships none" >&2; exit 1; }
command -v appimagetool >/dev/null || { echo "appimagetool not found on PATH" >&2; exit 1; }

appdir="$(mktemp -d)/OpenSpot.AppDir"
trap 'rm -rf "$(dirname "$appdir")"' EXIT
mkdir -p "$appdir/usr/bin"
cp "$bin_dir/openspot-gui" "$bin_dir/openspot" "$appdir/usr/bin/"
cp "$repo/packaging/linux/openspot.desktop" "$appdir/openspot.desktop"
cp "$icon" "$appdir/openspot.png"
ln -s usr/bin/openspot-gui "$appdir/AppRun"

mkdir -p "$repo/dist"
ARCH=x86_64 appimagetool "$appdir" "$repo/dist/openspot-v${version}-linux-x86_64.AppImage"
