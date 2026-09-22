# check-versions.ps1 — package / Cargo / tauri.conf and the newest CHANGELOG section must match.
# Usage: pwsh scripts/check-versions.ps1 [-ExpectedVersion "1.1.0"]
# When GITHUB_REF_NAME is v*, tag version must match package.json.

# NOTE: PowerShell variable names are case-INSENSITIVE — `$Expected` and `$expected` would be the
# same variable, silently discarding the bound parameter. Keep these names distinct.
param([string]$ExpectedVersion = "")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Read-JsonVersion([string]$path, [string]$prop = "version") {
    $obj = Get-Content -Raw $path | ConvertFrom-Json
    return [string]$obj.$prop
}

$pkg = Read-JsonVersion "package.json"
$cargoRaw = Get-Content -Raw "src-tauri/Cargo.toml"
if ($cargoRaw -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Cargo.toml version not found"
}
$cargo = $Matches[1]
$tauri = Read-JsonVersion "src-tauri/tauri.conf.json"

$errors = @()
if ($pkg -ne $cargo) { $errors += "package.json ($pkg) != Cargo.toml ($cargo)" }
if ($pkg -ne $tauri) { $errors += "package.json ($pkg) != tauri.conf.json ($tauri)" }

$expected = $null
if ($ExpectedVersion) { $expected = $ExpectedVersion }
if (-not $expected -and $env:GITHUB_REF_NAME -match '^v(.+)$') {
    $expected = $Matches[1]
}
if ($expected -and $pkg -ne $expected) {
    $errors += "version $pkg != expected $expected"
}

# The newest CHANGELOG section must be the released version.
if (Test-Path "CHANGELOG.md") {
    $cl = Get-Content "CHANGELOG.md"
    $head = $cl | Where-Object { $_ -match '^## \[(v?)([0-9]+\.[0-9]+\.[0-9]+)\]' } | Select-Object -First 1
    if (-not $head) {
        $errors += "CHANGELOG.md has no '## [x.y.z]' section"
    } elseif ($head -notmatch '\[([0-9]+\.[0-9]+\.[0-9]+)\]') {
        $errors += "CHANGELOG.md newest section unparsable: $head"
    } else {
        $clVer = $Matches[1]
        if ($clVer -ne $pkg) {
            $errors += "CHANGELOG newest section [$clVer] != package.json ($pkg)"
        }
    }
}

if ($errors.Count -gt 0) {
    Write-Host "check-versions FAILED:" -ForegroundColor Red
    $errors | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}

Write-Host "check-versions OK: $pkg (package / cargo / tauri)" -ForegroundColor Green
exit 0
