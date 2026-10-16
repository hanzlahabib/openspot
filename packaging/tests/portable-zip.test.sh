#!/usr/bin/env bash
# Tests the portable-zip packaging against dummy binaries: zip layout, byte-for-byte contents and
# SHA256SUMS. Runs build-portable.sh always, and build-portable.ps1 too when PowerShell 7 (pwsh) is
# available (set PWSH=/path/to/pwsh to point at one that is not on PATH).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
pwsh_bin="${PWSH:-$(command -v pwsh || true)}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

version="$(awk '/^\[workspace\.package\]/{f=1; next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/, "", $3); print $3; exit}' "$repo/Cargo.toml")"
[ -n "$version" ] || fail "could not read the workspace version"
name="openspot-v${version}-windows-x86_64-portable"

# Dummy "release binaries": distinct, non-trivial content so a mix-up cannot go unnoticed.
bin="$work/bin"
mkdir "$bin"
printf 'MZ not really a gui\n' > "$bin/openspot-gui.exe"
head -c 4096 /dev/urandom > "$bin/openspot.exe"

expected="$(printf '%s\n' \
  "$name/CHANGELOG.md" "$name/LICENSE-APACHE" "$name/LICENSE-MIT" "$name/README.md" \
  "$name/openspot-gui.exe" "$name/openspot.exe" | LC_ALL=C sort)"

# check_dist <label> <dist dir>
check_dist() {
  local label="$1" dist="$2" zip="$2/$name.zip"
  [ -f "$zip" ] || fail "$label: $zip was not created"

  local entries
  entries="$(unzip -Z1 "$zip")"
  case "$entries" in *\\*) fail "$label: entry names contain a backslash" ;; esac
  local files
  files="$(printf '%s\n' "$entries" | grep -v '/$' | LC_ALL=C sort)"
  [ "$files" = "$expected" ] || fail "$label: unexpected zip contents:
$files
expected:
$expected"
  printf '%s\n' "$entries" | grep -v "^$name/" && fail "$label: entry outside the top-level folder $name"

  local out="$work/extract-$label"
  mkdir "$out"
  unzip -q "$zip" -d "$out"
  cmp "$bin/openspot-gui.exe" "$out/$name/openspot-gui.exe" || fail "$label: openspot-gui.exe differs"
  cmp "$bin/openspot.exe" "$out/$name/openspot.exe" || fail "$label: openspot.exe differs"
  for f in README.md CHANGELOG.md LICENSE-APACHE LICENSE-MIT; do
    cmp "$repo/$f" "$out/$name/$f" || fail "$label: $f differs from the repository copy"
  done

  [ "$(wc -l < "$dist/SHA256SUMS")" -eq 1 ] || fail "$label: SHA256SUMS should list exactly the zip"
  grep -q "^[0-9a-f]\{64\}  $name.zip$" "$dist/SHA256SUMS" || fail "$label: SHA256SUMS line is not '<hex>  <name>'"
  (cd "$dist" && sha256sum -c --status SHA256SUMS) || fail "$label: sha256sum -c rejects SHA256SUMS"

  # A modified zip must fail verification, otherwise the check proves nothing.
  local bad="$work/tampered-$label"
  mkdir "$bad"
  cp "$zip" "$dist/SHA256SUMS" "$bad/"
  printf 'x' >> "$bad/$name.zip"
  (cd "$bad" && sha256sum -c --status SHA256SUMS) && fail "$label: a tampered zip still verified"
  echo "ok: $label"
}

"$repo/packaging/windows/build-portable.sh" "$bin" "$work/dist-bash" > /dev/null
check_dist bash "$work/dist-bash"

if [ -n "$pwsh_bin" ]; then
  # Parse every PowerShell script first: a syntax error elsewhere should fail loudly here.
  for script in "$repo"/packaging/windows/*.ps1; do
    "$pwsh_bin" -NoProfile -File "$here/parse-ps1.ps1" -Path "$script" \
      || fail "PowerShell parse errors in $script"
  done
  echo "ok: PowerShell scripts parse"
  "$pwsh_bin" -NoProfile -File "$repo/packaging/windows/build-portable.ps1" -BinDir "$bin" -OutDir "$work/dist-pwsh" > /dev/null
  check_dist pwsh "$work/dist-pwsh"
else
  echo "skipped: pwsh not found; the PowerShell scripts were not parsed or run"
fi
echo "all packaging tests passed"
