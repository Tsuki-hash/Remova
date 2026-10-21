# check-versions.ps1 — package / Cargo / tauri.conf / package-lock and the newest CHANGELOG section must match.
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
$cargoLockRaw = Get-Content -Raw "src-tauri/Cargo.lock"
if ($cargoLockRaw -notmatch '(?ms)^\[\[package\]\]\s*\r?\nname = "remova"\s*\r?\nversion = "([^"]+)"') {
    throw "Cargo.lock version not found"
}
$cargoLock = $Matches[1]

# package-lock.json carries the version twice and is a fourth source of truth.
# It was left at 1.3.1 while everything else moved to 1.3.2, and this gate did
# not look at it — so the drift this gate exists to catch happened silently, and
# `npm install` would rewrite the lock and make builds non-reproducible.
# Read through node rather than ConvertFrom-Json, which cannot deserialize a
# lock file of this depth.
$lockVersions = node -e "const l=require('./package-lock.json');process.stdout.write((l.version||'')+' '+(l.packages&&l.packages['']?l.packages[''].version:''))"
$lockParts = $lockVersions.Trim().Split(' ')
$lock = $lockParts[0]
$lockPkgVersion = if ($lockParts.Count -gt 1) { $lockParts[1] } else { "" }

$errors = @()
if ($pkg -ne $cargo) { $errors += "package.json ($pkg) != Cargo.toml ($cargo)" }
if ($pkg -ne $cargoLock) { $errors += "package.json ($pkg) != Cargo.lock ($cargoLock)" }
if ($pkg -ne $tauri) { $errors += "package.json ($pkg) != tauri.conf.json ($tauri)" }
if ($lock -ne $pkg) { $errors += "package-lock.json ($lock) != package.json ($pkg)" }
if ($lockPkgVersion -ne $pkg) {
  $errors += "package-lock.json packages[''].version ($lockPkgVersion) != package.json ($pkg)"
}

$expected = $null
if ($ExpectedVersion) { $expected = $ExpectedVersion }
if (-not $expected -and $env:GITHUB_REF_NAME -match '^v(.+)$') {
    $expected = $Matches[1]
}
if ($expected -and $pkg -ne $expected) {
    $errors += "version $pkg != expected $expected"
}

# The newest CHANGELOG section must be the released version. "Newest" is the
# highest semver in the file, not the first heading — taking the first meant
# that appending a new section below an older one still passed.
if (-not (Test-Path "CHANGELOG.md")) {
    $errors += "CHANGELOG.md is missing"
} else {
    $versions = @()
    foreach ($line in (Get-Content "CHANGELOG.md")) {
        if ($line -match '^## \[(v?)([0-9]+\.[0-9]+\.[0-9]+)\]') { $versions += $Matches[2] }
    }
    if ($versions.Count -eq 0) {
        $errors += "CHANGELOG.md has no '## [x.y.z]' section"
    } else {
        $latest = $versions | Sort-Object { [version]$_ } | Select-Object -Last 1
        if ($latest -ne $pkg) {
            $errors += "CHANGELOG newest section [$latest] != package.json ($pkg)"
        }
    }
}

if ($errors.Count -gt 0) {
    Write-Host "check-versions FAILED:" -ForegroundColor Red
    $errors | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}

Write-Host "check-versions OK: $pkg (package / cargo / cargo-lock / tauri / package-lock)" -ForegroundColor Green
exit 0
