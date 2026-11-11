$performanceKeys = @(
    'startup_worst_ms', 'idle_working_set_mb', 'walk_worst_ms',
    'analyze_worst_ms', 'cancel_worst_ms', 'backup_ms'
)

function Test-PerformanceNumber($Value) {
    ($Value -is [int] -or $Value -is [long] -or $Value -is [double] -or $Value -is [decimal]) -and
        [double]::IsFinite([double]$Value) -and $Value -gt 0
}

function Get-PerformanceThresholds([string]$Path) {
    $limits = [ordered]@{}
    # Parse the same snapshot that is fingerprinted, even if the file changes
    # later. Reading the hash and the table separately could pair different bytes.
    $bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path).Path)
    $sha = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
    foreach ($line in ([Text.Encoding]::UTF8.GetString($bytes) -split '\r?\n')) {
        if ($line -notmatch '^\|\s*[^|]+\|\s*([a-z0-9_]+)\s*\|\s*([^|]*)\|') { continue }
        $key, $raw = $Matches[1], $Matches[2].Trim()
        if ($key -cnotin $performanceKeys) { continue }
        if ($limits.Contains($key)) { throw "Duplicate performance threshold: $key" }
        $limit = 0.0
        if ($raw -notmatch '^[0-9]+(\.[0-9]+)?$' -or
            -not [double]::TryParse($raw, [Globalization.NumberStyles]::AllowDecimalPoint,
                [Globalization.CultureInfo]::InvariantCulture, [ref]$limit) -or
            -not (Test-PerformanceNumber $limit)) {
            throw "Invalid or unset performance threshold: $key"
        }
        $limits[$key] = $limit
    }
    $missing = @($performanceKeys | Where-Object { -not $limits.Contains($_) })
    if ($missing.Count) { throw "Missing performance thresholds: $($missing -join ', ')" }
    [pscustomobject]@{ limits = $limits; sha256 = $sha }
}

function Get-PerformanceEnvironment {
    $windows = Get-AcceptanceWindows
    $system = Get-CimInstance Win32_ComputerSystem
    $cpu = (Get-CimInstance Win32_Processor).Name -join ';'
    $fingerprint = "$($system.Name)|$($system.Model)|$($system.TotalPhysicalMemory)|$cpu|$($windows.version)"
    [ordered]@{
        machine_sha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData(
            [Text.Encoding]::UTF8.GetBytes($fingerprint))).ToLowerInvariant()
        os_version = $windows.version
        architecture = $windows.architecture
        webview2_version = Get-AcceptanceWebViewVersion
    }
}

function Assert-PerformanceIdentity($Record, [string]$Kind, [string]$Commit, [string]$ExeSha, [string]$ThresholdSha) {
    if (-not (Test-PerformanceNumber $Record.schema_version) -or $Record.schema_version -ne 1 -or
        $Record.kind -cne $Kind -or
        $Record.candidate_commit -ine $Commit -or $Record.sha256 -ine $ExeSha -or
        $Record.thresholds_sha256 -ine $ThresholdSha) {
        throw "The $Kind evidence does not match the expected candidate commit, executable or thresholds."
    }
    if ($Record.environment.machine_sha256 -notmatch '^[0-9a-f]{64}$' -or
        $Record.environment.os_version -notmatch '^10\.0\.[0-9]+(\.[0-9]+)?$' -or
        $Record.environment.architecture -cne 'x64' -or
        $Record.environment.webview2_version -notmatch '^[1-9]\d*\.\d+\.\d+\.\d+$') {
        throw "The $Kind evidence lacks a supported measurement environment."
    }
}

function Get-StartupMetrics($Record) {
    $samples = @($Record.runs)
    if ($samples.Count -lt 3) { throw 'At least three startup runs are required.' }
    for ($i = 0; $i -lt $samples.Count; $i++) {
        $sample = $samples[$i]
        if (-not (Test-PerformanceNumber $sample.run) -or $sample.run -ne ($i + 1) -or
            $sample.ready_observed -isnot [bool] -or -not $sample.ready_observed -or
            -not (Test-PerformanceNumber $sample.startup_ms) -or -not (Test-PerformanceNumber $sample.working_set_mb)) {
            throw "Startup run $($i + 1) lacks a ready window or valid timing/memory data."
        }
    }
    @{
        startup_worst_ms = ($samples.startup_ms | Measure-Object -Maximum).Maximum
        idle_working_set_mb = ($samples.working_set_mb | Measure-Object -Maximum).Maximum
    }
}
