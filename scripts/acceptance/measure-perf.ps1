#!/usr/bin/env pwsh
# Measure startup and idle memory from a candidate executable.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{40}$')][string]$CandidateCommit,
    [string]$Thresholds = '',
    [ValidateRange(3, 20)][int]$Runs = 3,
    [switch]$CollectOnly
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance-common.ps1"
. "$PSScriptRoot/performance-common.ps1"
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Thresholds) { $Thresholds = Join-Path $repoRoot 'benchmarks/P-01-thresholds.md' }

try {
    $threshold = Get-PerformanceThresholds $Thresholds
    $Exe = (Get-Item -LiteralPath $Exe).FullName
    if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { throw "Executable not found: $Exe" }
    if (Get-Process -Name 'Remova', ([IO.Path]::GetFileNameWithoutExtension($Exe)) -ErrorAction SilentlyContinue) {
        throw 'Close every running instance of the candidate before measuring startup.'
    }
    $environment = Get-PerformanceEnvironment
    $sha = (Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash.ToLowerInvariant()
    New-Item -ItemType Directory -Force -Path $Out | Out-Null
} catch {
    Write-Host "Evidence insufficient: $($_.Exception.Message)" -ForegroundColor Yellow
    exit 2
}

$samples = [System.Collections.Generic.List[object]]::new()
$result = [ordered]@{
    schema_version = 1
    kind = 'remova-startup'
    executable = $Exe
    candidate_commit = $CandidateCommit.ToLowerInvariant()
    sha256 = $sha
    thresholds_sha256 = $threshold.sha256
    environment = $environment
    runs = $samples
}
$exitCode = 2
try {
    for ($i = 1; $i -le $Runs; $i++) {
        $startup = [Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process -FilePath $Exe -PassThru
        try {
            $ready = Wait-RemovaUiReady $process
            $startup.Stop()
            $sample = [ordered]@{
                run = $i
                ready_observed = $ready
                startup_ms = $startup.Elapsed.TotalMilliseconds
                working_set_mb = $null
            }
            $samples.Add($sample)
            if (-not $ready) { throw "The candidate UI did not become ready in run $i." }

            # Sample the Remova process's idle working set after UI readiness.
            $idle = [Diagnostics.Stopwatch]::StartNew()
            $peak = 0
            while ($idle.Elapsed.TotalSeconds -lt 2) {
                $process.Refresh()
                if ($process.HasExited) { throw "The candidate quit while sampling run $i." }
                $peak = [math]::Max($peak, $process.WorkingSet64)
                Start-Sleep -Milliseconds 100
            }
            $sample.working_set_mb = $peak / 1MB
        } finally {
            Close-RemovaTestProcess $process
        }
    }
    if ((Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash -ine $sha) {
        throw 'The executable changed during measurement.'
    }
    $metrics = Get-StartupMetrics ([pscustomobject]$result)
    foreach ($key in $metrics.Keys) { $result[$key] = $metrics[$key] }
    if (-not $CollectOnly) {
        $exitCode = 0
        foreach ($key in $metrics.Keys) {
            $limit = $threshold.limits[$key]
            $actual = $metrics[$key]
            $within = $actual -le $limit
            $status = if ($within) { 'PASS' } else { 'FAIL' }
            Write-Host "$key = $actual (limit $limit) $status"
            if (-not $within) { $exitCode = 1 }
        }
    }
} catch {
    $result.error = $_.Exception.Message
    Write-Host "Evidence insufficient: $($result.error)" -ForegroundColor Yellow
} finally {
    $result.measured_at = (Get-Date).ToUniversalTime().ToString('o')
    $result.collect_only = [bool]$CollectOnly
    $file = Join-Path $Out 'P-01-startup.json'
    $result | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $file -Encoding utf8
    Write-Host "Wrote $file"
}
if ($CollectOnly) { Write-Host 'Startup data collected without a gate verdict.' -ForegroundColor Yellow }
exit $exitCode
