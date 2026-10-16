# Parses a PowerShell script without running it and exits 1 on syntax errors.
param([Parameter(Mandatory)][string]$Path)
$errors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path -LiteralPath $Path).Path, [ref]$null, [ref]$errors)
if ($errors) {
    $errors | ForEach-Object { Write-Host "$Path`:$($_.Extent.StartLineNumber): $($_.Message)" }
    exit 1
}
