#!/usr/bin/env pwsh
# Measure startup and idle memory from a candidate executable.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{40}$')][string]$CandidateCommit,
    [string]$Thresholds = '',
    [ValidateSet('cold','warm')][string]$Mode = 'warm',
    [ValidateRange(1, 20)][int]$Runs = 3,
    [switch]$ConfirmFirstLaunchSinceBoot,
    [switch]$CollectOnly
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance-common.ps1"
. "$PSScriptRoot/performance-common.ps1"
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Thresholds) { $Thresholds = Join-Path $repoRoot 'benchmarks/performance-thresholds.md' }

$samples = [System.Collections.Generic.List[object]]::new()
$boot = $null

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
    $file = Join-Path $Out "startup-$Mode.json"
    if ($Mode -ceq 'cold') {
        if (!$ConfirmFirstLaunchSinceBoot -or ($PSBoundParameters.ContainsKey('Runs') -and $Runs -ne 1)) {
            throw 'Record one cold sample per Windows boot and confirm that Remova has not been launched since rebooting.'
        }
        $Runs = 1
        $bootTime = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime
        if ($bootTime -isnot [datetime]) { throw 'Could not identify the current Windows boot.' }
        $identity = "$($environment.machine_sha256)|$($bootTime.ToUniversalTime().Ticks)"
        $boot = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData(
            [Text.Encoding]::UTF8.GetBytes($identity))).ToLowerInvariant()
        if (Test-Path -LiteralPath $file) {
            $previous = Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
            Assert-PerformanceIdentity $previous 'remova-startup' $CandidateCommit $sha $threshold.sha256
            if ($previous.error) { throw 'Keep the failed measurement record and use a new output directory for a new series.' }
            foreach ($key in $environment.Keys) {
                if ($environment[$key] -cne $previous.environment.$key) {
                    throw "Cold startup environment changed for $key."
                }
            }
            $null = Get-StartupMetrics $previous 'cold' 1
            if ($boot -cin @($previous.runs.boot_sha256) -or @($previous.runs).Count -ge 20) {
                throw 'The cold startup series already has a sample from this boot or is full.'
            }
            foreach ($sample in $previous.runs) { $samples.Add($sample) }
        }
    } elseif ($Runs -lt 3) {
        throw 'Collect at least three runs for a warm startup series.'
    }
} catch {
    Write-Host "Evidence insufficient: $($_.Exception.Message)" -ForegroundColor Yellow
    exit 2
}

$result = [ordered]@{
    schema_version = 1
    kind = 'remova-startup'
    executable = $Exe
    candidate_commit = $CandidateCommit.ToLowerInvariant()
    sha256 = $sha
    thresholds_sha256 = $threshold.sha256
    environment = $environment
    measurement_mode = $Mode
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
                run = $samples.Count + 1
                boot_sha256 = $boot
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
    $metrics = Get-StartupMetrics ([pscustomobject]$result) $Mode 1
    foreach ($key in $metrics.Keys) { $result[$key] = $metrics[$key] }
    if (-not $CollectOnly -and $Mode -ceq 'cold') {
        $exitCode = 0
        foreach ($key in $metrics.Keys) {
            $limit = $threshold.limits[$key]
            $actual = $metrics[$key]
            $within = $actual -le $limit
            $status = if ($within) { 'PASS' } else { 'FAIL' }
            Write-Host "$key = $actual (limit $limit) $status"
            if (-not $within) { $exitCode = 1 }
        }
        if ($exitCode -eq 0 -and $samples.Count -lt 3) {
            Write-Host 'Collect a sample after each of three separate Windows boots.' -ForegroundColor Yellow
            $exitCode = 2
        }
    }
} catch {
    $result.error = $_.Exception.Message
    Write-Host "Evidence insufficient: $($result.error)" -ForegroundColor Yellow
} finally {
    $result.measured_at = (Get-Date).ToUniversalTime().ToString('o')
    $result.collect_only = [bool]$CollectOnly
    $pending = "$file.$([guid]::NewGuid().ToString('N')).tmp"
    try {
        $result | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $pending -Encoding utf8
        Move-Item -LiteralPath $pending -Destination $file -Force
    } finally {
        if (Test-Path -LiteralPath $pending) { Remove-Item -LiteralPath $pending }
    }
    Write-Host "Wrote $file"
}
if ($CollectOnly -or $Mode -ceq 'warm') { Write-Host 'Startup data collected without a release verdict.' -ForegroundColor Yellow }
exit $exitCode
