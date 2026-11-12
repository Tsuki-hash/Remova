#!/usr/bin/env pwsh
# Judge startup and component evidence against all six agreed thresholds.
# 0 = all six pass, 1 = threshold exceeded, 2 = evidence insufficient.
param(
    [string]$Components = '',
    [Parameter(Mandatory = $true)][string]$Startup,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{40}$')][string]$CandidateCommit,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{64}$')][string]$CandidateExeSha256,
    [string]$Thresholds = '',
    [switch]$Run,
    [switch]$AllowPartial
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance-common.ps1"
. "$PSScriptRoot/performance-common.ps1"
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Thresholds) { $Thresholds = Join-Path $repoRoot 'benchmarks/performance-thresholds.md' }

try {
    $threshold = Get-PerformanceThresholds $Thresholds
    $startupRecord = Get-Content -LiteralPath $Startup -Raw | ConvertFrom-Json
    Assert-PerformanceIdentity $startupRecord 'remova-startup' $CandidateCommit $CandidateExeSha256 $threshold.sha256
    if ($startupRecord.error) { throw "Startup collection failed: $($startupRecord.error)" }
    $metrics = Get-StartupMetrics $startupRecord

    if ($Run) {
        $head = & git -C $repoRoot rev-parse HEAD
        if ($LASTEXITCODE -ne 0 -or $head -ine $CandidateCommit) { throw 'Check out the candidate commit before measuring components.' }
        & git -C $repoRoot diff --quiet HEAD --
        if ($LASTEXITCODE -ne 0) { throw 'Component measurement requires an unchanged candidate checkout.' }
        $untracked = & git -C $repoRoot ls-files --others --exclude-standard -- src-tauri src public scripts
        if ($LASTEXITCODE -ne 0 -or $untracked) { throw 'Untracked application or script source prevents candidate identification.' }
        $environment = Get-PerformanceEnvironment
        foreach ($key in $environment.Keys) {
            if ($environment[$key] -cne $startupRecord.environment.$key) {
                throw "Component and startup environments differ for $key."
            }
        }

        $runName = 'perf-{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 8))
        $outDir = Join-Path $repoRoot "benchmarks/runs/$runName"
        New-Item -ItemType Directory -Path $outDir | Out-Null
        Write-Host "Running the release baseline; output goes to $outDir"
        Push-Location (Join-Path $repoRoot 'src-tauri')
        $previousOut = $env:REMOVA_PERF_OUT
        try {
            $env:REMOVA_PERF_OUT = $outDir
            & cargo test --locked --release --lib perf_baseline -- --ignored --nocapture --test-threads=1
            if ($LASTEXITCODE -ne 0) { throw "perf_baseline failed with exit code $LASTEXITCODE" }
        } finally {
            $env:REMOVA_PERF_OUT = $previousOut
            Pop-Location
        }
        $headAfter = & git -C $repoRoot rev-parse HEAD
        if ($LASTEXITCODE -ne 0 -or $headAfter -ine $CandidateCommit) { throw 'Candidate commit changed during component measurement.' }
        & git -C $repoRoot diff --quiet HEAD --
        if ($LASTEXITCODE -ne 0) { throw 'Candidate checkout changed during component measurement.' }
        $Components = Join-Path $outDir 'components.json'
        $document = Get-Content -LiteralPath $Components -Raw | ConvertFrom-Json -AsHashtable
        if ($document.kind -cne 'remova-components') { throw 'The component test did not emit a measurement.' }
        $document.schema_version = 1
        $document.candidate_commit = $CandidateCommit.ToLowerInvariant()
        $document.sha256 = $CandidateExeSha256.ToLowerInvariant()
        $document.thresholds_sha256 = $threshold.sha256
        $document.environment = $environment
        $document | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $Components -Encoding utf8
    }

    if (-not $Components) { throw 'Pass -Components explicitly, or measure the current commit with -Run.' }
    $componentRecord = Get-Content -LiteralPath $Components -Raw | ConvertFrom-Json
    Assert-PerformanceIdentity $componentRecord 'remova-components' $CandidateCommit $CandidateExeSha256 $threshold.sha256
    foreach ($key in @('machine_sha256', 'os_version', 'architecture', 'webview2_version', 'elevated')) {
        if ($componentRecord.environment.$key -cne $startupRecord.environment.$key) {
            throw "Component and startup environments differ for $key."
        }
    }
    if ($componentRecord.fixture.walk_entries -ne 4040 -or $componentRecord.fixture.analyze_files -ne 200 -or
        $componentRecord.fixture.backup_bytes -ne 1GB) {
        throw 'The component workload does not match the agreed fixture sizes.'
    }

    $missing = @()
    $failed = $false
    foreach ($key in $performanceKeys) {
        $actual = if ($metrics.ContainsKey($key)) { $metrics[$key] } else { $componentRecord.metrics.$key }
        $limit = $threshold.limits[$key]
        if (-not (Test-PerformanceNumber $actual)) {
            $missing += $key
            continue
        }
        $within = $actual -le $limit
        $status = if ($within) { 'PASS' } else { 'FAIL' }
        Write-Host "$key = $actual (limit $limit) $status"
        if (-not $within) { $failed = $true }
    }
    if ($missing.Count) { throw "Missing or invalid metric values: $($missing -join ', ')" }
    if ($failed) { exit 1 }
    Write-Host "All six performance thresholds passed for $CandidateCommit ($CandidateExeSha256)." -ForegroundColor Green
    exit 0
} catch {
    Write-Host "Evidence insufficient: $($_.Exception.Message)" -ForegroundColor Yellow
    if ($AllowPartial) { Write-Host 'Partial measurements do not pass the gate.' }
    exit 2
}
