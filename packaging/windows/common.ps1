# Helpers shared by build-portable.ps1 and build-installer.ps1 (dot-sourced).
Set-StrictMode -Version Latest

function Get-WorkspaceVersion {
    <# Version from [workspace.package] in the root Cargo.toml: the single source of truth. #>
    param([Parameter(Mandatory)][string]$RepoRoot)
    $toml = Get-Content -Raw -LiteralPath (Join-Path $RepoRoot 'Cargo.toml')
    $m = [regex]::Match($toml, '(?ms)^\[workspace\.package\]\s*$.*?^version\s*=\s*"([^"]+)"')
    if (-not $m.Success) { throw "no version under [workspace.package] in $RepoRoot/Cargo.toml" }
    return $m.Groups[1].Value
}

function Write-Checksums {
    <# SHA256SUMS in `sha256sum` format ("<hex>  <name>", LF line ends, no BOM) so that
       `sha256sum -c` and `shasum -a 256 -c` accept it on any platform. #>
    param([Parameter(Mandatory)][string]$Directory)
    $lines = Get-ChildItem -LiteralPath $Directory -File |
        Where-Object { $_.Name -ne 'SHA256SUMS' } |
        Sort-Object Name |
        ForEach-Object { '{0}  {1}' -f (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant(), $_.Name }
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText((Join-Path $Directory 'SHA256SUMS'), (($lines -join "`n") + "`n"), $utf8NoBom)
}
