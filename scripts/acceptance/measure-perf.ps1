#!/usr/bin/env pwsh
# Performance measurement harness.
#
# Records timings and peak working set for the candidate build, then compares
# them against the thresholds a maintainer filled in beforehand. It refuses to
# declare a pass while any threshold is still "待填": a measurement without a
# pre-agreed bound cannot prove "no regression beyond the threshold".
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Thresholds = '',
    [int]$Runs = 3
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($Thresholds)) {
    # scripts/acceptance -> repository root
    $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    $Thresholds = Join-Path (Join-Path $repoRoot 'benchmarks') 'P-01-thresholds.md'
}

if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { throw "Executable not found: $Exe" }
if (-not (Test-Path -LiteralPath $Thresholds -PathType Leaf)) { throw "Threshold file not found: $Thresholds" }
New-Item -ItemType Directory -Force -Path $Out | Out-Null

# --- Refuse to measure without pre-agreed thresholds ------------------------
$thresholdText = Get-Content -LiteralPath $Thresholds -Raw
$pending = ([regex]::Matches($thresholdText, '待填')).Count
if ($pending -gt 0) {
    throw "The threshold file still has $pending unfilled values. A maintainer must set the bounds BEFORE measuring; otherwise the result cannot be judged."
}

# --- Collect runs ----------------------------------------------------------
$sha = (Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash.ToLowerInvariant()
$runs = @()
for ($i = 1; $i -le $Runs; $i++) {
    $proc = Start-Process -FilePath $Exe -PassThru
    try {
        # Time to a responsive main window; WaitForInputIdle returns false on a
        # console host, so measure wall clock to first idle as the proxy and
        # record whether it was actually observed.
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $idle = $proc.WaitForInputIdle(30000)
        $sw.Stop()
        Start-Sleep -Seconds 2
        $proc.Refresh()
        $runs += [ordered]@{
            run          = $i
            idle_observed = $idle
            startup_ms   = [int]$sw.ElapsedMilliseconds
            working_set_mb = [math]::Round($proc.WorkingSet64 / 1MB, 1)
            private_mb   = [math]::Round($proc.PrivateMemorySize64 / 1MB, 1)
        }
    } finally {
        if (-not $proc.HasExited) { $proc.Kill(); $proc.WaitForExit(10000) | Out-Null }
    }
}

$startupValues = $runs | ForEach-Object { $_.startup_ms } | Sort-Object
$median = $startupValues[[int][math]::Floor($startupValues.Count / 2)]
$worst = $startupValues[-1]

$result = [ordered]@{
    executable   = (Resolve-Path $Exe).Path
    sha256       = $sha
    measured_at  = (Get-Date).ToString('o')
    runs         = $runs
    startup_median_ms = $median
    startup_worst_ms  = $worst
    machine      = [ordered]@{
        cpu  = (Get-CimInstance Win32_Processor).Name
        ram_mb = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1MB)
        os   = (Get-CimInstance Win32_OperatingSystem).Caption
        os_version = (Get-CimInstance Win32_OperatingSystem).Version
    }
    thresholds_file = (Resolve-Path $Thresholds).Path
}
$file = Join-Path $Out 'P-01-startup.json'
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $file -Encoding utf8
Write-Host "Wrote $file"
Write-Host ("Startup median {0} ms, worst {1} ms over {2} runs" -f $median, $worst, $Runs)

# --- Judgement -------------------------------------------------------------
# Only the startup metric is automated here; the remaining rows in the
# threshold file need the scan/backup fixtures described in
# scripts/acceptance/README.md and must be filled by the operator.
$limitMatch = [regex]::Match($thresholdText, '(?m)^\|\s*冷启动到可交互\s*\|\s*([0-9]+)\s*(ms|s)?\s*\|')
if ($limitMatch.Success) {
    $limit = [int]$limitMatch.Groups[1].Value
    if ($limitMatch.Groups[2].Value -eq 's') { $limit *= 1000 }
    if ($worst -gt $limit) {
        Write-Host ("FAIL: worst startup {0} ms exceeds the agreed {1} ms" -f $worst, $limit) -ForegroundColor Red
        exit 1
    }
    Write-Host ("PASS: worst startup {0} ms within the agreed {1} ms" -f $worst, $limit) -ForegroundColor Green
} else {
    Write-Host 'The startup row in the threshold file could not be parsed; fill it as an integer (ms or s).' -ForegroundColor Yellow
    exit 1
}
