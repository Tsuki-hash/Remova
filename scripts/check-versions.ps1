# check-versions.ps1 — package / Cargo / tauri.conf (and optional docs index) must match.
# Usage: pwsh scripts/check-versions.ps1 [-Expected "1.1.0"]
# When GITHUB_REF_NAME is v*, tag version must match package.json.

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
if ($args.Count -ge 1 -and $args[0]) { $expected = $args[0] }
if (-not $expected -and $env:GITHUB_REF_NAME -match '^v(.+)$') {
    $expected = $Matches[1]
}
if ($expected -and $pkg -ne $expected) {
    $errors += "version $pkg != expected $expected"
}

# docs/README.md may state current version — warn only if present and mismatched
$docsReadme = "docs/README.md"
if (Test-Path $docsReadme) {
    $txt = Get-Content -Raw $docsReadme
    if ($txt -match '当前版本\s*\*\*([0-9]+\.[0-9]+\.[0-9]+)\*\*') {
        $docsVer = $Matches[1]
        if ($docsVer -ne $pkg) {
            $errors += "docs/README.md ($docsVer) != package.json ($pkg)"
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
