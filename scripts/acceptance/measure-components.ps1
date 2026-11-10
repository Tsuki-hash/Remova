#!/usr/bin/env pwsh
# Judge the component performance baseline against the agreed thresholds.
#
# The measurement itself lives in the crate: `perf_baseline` is an #[ignore]d
# test so it never runs beside the ordinary suite. This script runs it in
# release mode (or reads an earlier JSON), then applies the thresholds file.
#
# Exit codes: 0 = every measured row within its bound; 1 = a bound was exceeded;
# 2 = data missing (evidence insufficient, which is not a pass).
param(
    [string]$Components = '',
    [string]$Startup = '',
    [string]$Thresholds = '',
    [switch]$Run,
    # Collect/compare only the rows that have data. Still never turns a missing
    # row into a pass: it is reported as "未测".
    [switch]$AllowPartial
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

if ([string]::IsNullOrWhiteSpace($Thresholds)) {
    $Thresholds = Join-Path (Join-Path $repoRoot 'benchmarks') 'P-01-thresholds.md'
}
if (-not (Test-Path -LiteralPath $Thresholds -PathType Leaf)) { throw "Threshold file not found: $Thresholds" }

function New-RunDir {
    $stamp = (Get-Date).ToString('yyyyMMdd-HHmmss')
    $dir = Join-Path (Join-Path $repoRoot 'benchmarks') "runs/perf-$stamp"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $dir
}

if ($Run) {
    $outDir = New-RunDir
    Write-Host "Running the release baseline; output goes to $outDir"
    Push-Location (Join-Path $repoRoot 'src-tauri')
    try {
        $env:REMOVA_PERF_OUT = $outDir
        & cargo test --release --lib perf_baseline -- --ignored --nocapture --test-threads=1
        # Never let a piped command hide a failed measurement.
        if ($LASTEXITCODE -ne 0) { throw "perf_baseline failed with exit code $LASTEXITCODE" }
    } finally {
        Remove-Item Env:REMOVA_PERF_OUT -ErrorAction SilentlyContinue
        Pop-Location
    }
    $Components = Join-Path $outDir 'components.json'
}

if ([string]::IsNullOrWhiteSpace($Components)) {
    $latest = Get-ChildItem -Path (Join-Path $repoRoot 'benchmarks') -Recurse -Filter 'components.json' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $latest) { throw 'No components.json found. Run with -Run, or pass -Components.' }
    $Components = $latest.FullName
}
# Startup: judge the worst value across every recorded run (cold and warm), so a
# fast warm run cannot hide the cold-start number.
$startupFiles = @(Get-ChildItem -Path (Join-Path $repoRoot 'benchmarks') -Recurse -Filter 'P-01-startup.json' -ErrorAction SilentlyContinue)
$coldStartup = $startupFiles | Where-Object { $_.FullName -notmatch 'warm' } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($Startup) -and $coldStartup) { $Startup = $coldStartup.FullName }

# PowerShell variable names are case-insensitive: `$components` would overwrite
# `$Components` (the path). Keep the parsed JSON under a distinct name.
$perf = Get-Content -LiteralPath $Components -Raw | ConvertFrom-Json
$startupJson = if ($Startup -and (Test-Path -LiteralPath $Startup -PathType Leaf)) {
    Get-Content -LiteralPath $Startup -Raw | ConvertFrom-Json
} else { $null }

# Startup and idle memory come from the GUI harness; the component test covers
# the scan/cancel/backup paths.
function Get-Metric([string]$key) {
    switch ($key) {
        'startup_worst_ms' { if ($startupJson) { return [double]$startupJson.startup_worst_ms } }
        'idle_working_set_mb' {
            if ($startupJson -and $startupJson.runs) {
                return [double](($startupJson.runs | ForEach-Object { $_.working_set_mb } | Measure-Object -Maximum).Maximum)
            }
        }
        default { if ($perf.metrics.PSObject.Properties.Name -contains $key) { return [double]$perf.metrics.$key } }
    }
    return $null
}

$rows = @()
$failed = $false
$missing = @()
foreach ($line in Get-Content -LiteralPath $Thresholds) {
    $m = [regex]::Match($line, '^\|\s*(?<label>[^|]+?)\s*\|\s*(?<key>[a-z0-9_]+)\s*\|\s*(?<limit>[0-9]+)\s*\|')
    if (-not $m.Success) { continue }
    $label = $m.Groups['label'].Value
    $key = $m.Groups['key'].Value
    $limit = [double]$m.Groups['limit'].Value
    $value = Get-Metric $key
    if ($null -eq $value) {
        $missing += $label
        $rows += [pscustomobject]@{ 指标 = $label; 实测 = '未测'; 上限 = $limit; 结论 = '证据不足' }
        continue
    }
    $ok = $value -le $limit
    if (-not $ok) { $failed = $true }
    $rows += [pscustomobject]@{ 指标 = $label; 实测 = $value; 上限 = $limit; 结论 = $(if ($ok) { 'PASS' } else { 'FAIL' }) }
}

$rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host "components: $Components"
if ($Startup) { Write-Host "startup:    $Startup" }

if ($failed) {
    Write-Host 'Baseline NOT met: at least one measured row exceeds its agreed bound.' -ForegroundColor Red
    exit 1
}
if ($missing.Count -gt 0) {
    Write-Host ("Baseline incomplete: no data for {0}." -f ($missing -join '、')) -ForegroundColor Yellow
    if (-not $AllowPartial) { exit 2 }
}
Write-Host 'Baseline: every measured row is within its agreed bound.' -ForegroundColor Green
exit 0
