<#
.SYNOPSIS
  Compiles openspot.iss with Inno Setup into openspot-v<version>-windows-x86_64-setup.exe.

.DESCRIPTION
  Reads the version from Cargo.toml and passes it to the script. Needs Inno Setup 6 (ISCC.exe).
  Optionally signs the installer and uninstaller: pass -SignCommand with `$f` where the file
  name goes, for example (see SIGNING.md):
    -SignCommand 'signtool.exe sign /fd SHA256 /td SHA256 /tr http://timestamp.acs.microsoft.com /a $f'

  UNVERIFIED: never run on Windows. Only the PowerShell syntax has been parsed.
#>
[CmdletBinding()]
param(
    [string]$Target = 'x86_64-pc-windows-msvc',
    [string]$BinDir,
    [string]$OutDir = 'dist',
    [string]$Iscc,
    [string]$SignCommand
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $BinDir) { $BinDir = Join-Path (Join-Path (Join-Path $repo 'target') $Target) 'release' }
$version = Get-WorkspaceVersion -RepoRoot $repo

if (-not $Iscc) {
    $candidates = @(
        (Get-Command ISCC.exe -ErrorAction SilentlyContinue | ForEach-Object Source),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
    $Iscc = $candidates | Select-Object -First 1
}
if (-not $Iscc) { throw 'ISCC.exe not found; install Inno Setup 6 or pass -Iscc.' }
foreach ($p in 'openspot.exe', 'openspot-gui.exe') {
    if (-not (Test-Path -LiteralPath (Join-Path $BinDir $p))) { throw "missing $p in $BinDir; build the release binaries first" }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path
$isccArgs = @("/DAppVersion=$version", "/DBinDir=$BinDir", "/DRepoRoot=$repo", "/DOutDir=$OutDir")
if ($SignCommand) { $isccArgs += @('/DSignToolEnabled', "/Sopenspotsign=$SignCommand") }
$isccArgs += (Join-Path $PSScriptRoot 'openspot.iss')

& $Iscc @isccArgs
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
Write-Checksums -Directory $OutDir
Write-Host "installer written to $OutDir"
