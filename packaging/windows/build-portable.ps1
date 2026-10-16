<#
.SYNOPSIS
  Builds the portable zip and SHA256SUMS from release binaries.

.DESCRIPTION
  Expects `openspot.exe` and `openspot-gui.exe` in -BinDir (default: target\<Target>\release,
  produced by `cargo build --release --target <Target> -p openspot-cli -p openspot-app`).
  Writes <OutDir>\openspot-v<version>-windows-x86_64-portable.zip and <OutDir>\SHA256SUMS.
  The version comes from [workspace.package] in Cargo.toml.

  "Portable" means no installer and nothing written next to the program. Preferences and
  aliases still live in %APPDATA%\OpenSpot, as with the installed version.

  UNVERIFIED on Windows PowerShell 5.1. Run on PowerShell 7 on Linux against dummy binaries
  by packaging/tests/portable-zip.test.sh.
#>
[CmdletBinding()]
param(
    [string]$Target = 'x86_64-pc-windows-msvc',
    [string]$BinDir,
    [string]$OutDir = 'dist'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
. (Join-Path $PSScriptRoot 'common.ps1')

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $BinDir) { $BinDir = Join-Path (Join-Path (Join-Path $repo 'target') $Target) 'release' }
$version = Get-WorkspaceVersion -RepoRoot $repo
$name = "openspot-v$version-windows-x86_64-portable"

$programs = 'openspot.exe', 'openspot-gui.exe'
foreach ($p in $programs) {
    if (-not (Test-Path -LiteralPath (Join-Path $BinDir $p))) { throw "missing $p in $BinDir; build the release binaries first" }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path
$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("openspot-stage-" + [guid]::NewGuid().ToString('N'))
$root = Join-Path $stage $name
New-Item -ItemType Directory -Force -Path $root | Out-Null
try {
    foreach ($p in $programs) { Copy-Item -LiteralPath (Join-Path $BinDir $p) -Destination $root }
    foreach ($f in 'README.md', 'CHANGELOG.md', 'LICENSE-APACHE', 'LICENSE-MIT') {
        Copy-Item -LiteralPath (Join-Path $repo $f) -Destination $root
    }
    $zip = Join-Path $OutDir "$name.zip"
    if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip }
    # The staging directory holds one folder, $name, and is zipped without its own name, so the
    # archive has exactly one top-level folder and extracting does not scatter files around.
    [System.IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip, [System.IO.Compression.CompressionLevel]::Optimal, $false)
} finally {
    Remove-Item -Recurse -Force -LiteralPath $stage -ErrorAction SilentlyContinue
}
Write-Checksums -Directory $OutDir
Write-Host "wrote $zip"
