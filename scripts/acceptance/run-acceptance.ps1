#!/usr/bin/env pwsh
# Record isolated-VM observations and the identity of the three tested packages.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('client', 'elevated')][string]$Suite,
    [string]$Artifacts = $env:REMOVA_ACCEPTANCE_ARTIFACTS,
    [string]$Out = $env:REMOVA_ACCEPTANCE_OUT,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{40}$')][string]$CandidateCommit,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+$')][string]$CandidateVersion
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance-common.ps1"

if (-not $Artifacts -or -not (Test-Path -LiteralPath $Artifacts -PathType Container)) {
    throw 'Set -Artifacts to the directory with the three release packages and SHA256SUMS.txt.'
}
if (-not $Out) { throw 'Set -Out to a directory for the observations.' }

$hostModel = Confirm-AcceptanceVm
$isElevated = Test-AcceptanceElevated
if ($isElevated -ne ($Suite -eq 'elevated')) {
    throw "The current Windows token does not match the $Suite suite."
}
$windows = Get-AcceptanceWindows
$webview = Get-AcceptanceWebViewVersion
$identity = Get-AcceptanceArtifacts $Artifacts $CandidateVersion
$records = [System.Collections.Generic.List[object]]::new()
foreach ($check in $identity.checks) { $records.Add($check) }

$manual = @(Get-AcceptanceChecks $Suite | Where-Object { $_.requires_evidence })

$runName = '{0}-{1}-{2}-{3}' -f $windows.family, $Suite, (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 8))
$runDir = Join-Path $Out $runName
New-Item -ItemType Directory -Path $runDir | Out-Null
$summary = [ordered]@{
    schema_version = 1
    kind = 'remova-acceptance'
    candidate_commit = $CandidateCommit.ToLowerInvariant()
    candidate_version = $CandidateVersion
    suite = $Suite
    elevated = $isElevated
    os_family = $windows.family
    os_version = $windows.version
    architecture = $windows.architecture
    webview2_version = $webview
    host_model = $hostModel
    executor = $env:USERNAME
    artifacts = $identity.artifacts
    checks = $records
    passed = $false
}

try {
    if (@($records | Where-Object { $_.status -ne 'pass' }).Count) {
        throw 'The candidate packages do not match the release checksums.'
    }
    foreach ($check in $manual) {
        $status = Read-Host "$($check.id): $($check.expected). Observed PASS, FAIL or skip"
        $actual = ''
        $path = ''
        if ($status.Trim() -in @('pass', 'fail')) {
            $actual = Read-Host 'Describe the actual result, including the fixture used'
            $path = Read-Host 'Path to a log or screenshot showing this result'
        }
        $observation = New-AcceptanceObservation $check $status $actual $path
        if ($observation.evidence) {
            # Preserve the exact evidence that was reviewed and hashed.
            $evidenceName = "$($check.id)-$($observation.evidence.name)"
            $destination = Join-Path $runDir $evidenceName
            Copy-Item -LiteralPath $path -Destination $destination
            if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ine $observation.evidence.sha256) {
                $observation.status = 'fail'
                $observation.actual = 'The evidence changed while it was copied.'
                $observation.Remove('evidence')
            } else {
                $observation.evidence.name = $evidenceName
            }
        }
        $records.Add($observation)
        Write-Host "$($observation.id): $($observation.status)"
    }
    $finalIdentity = Get-AcceptanceArtifacts $Artifacts $CandidateVersion
    foreach ($channel in $identity.artifacts.Keys) {
        if ($finalIdentity.artifacts[$channel].sha256 -cne $identity.artifacts[$channel].sha256) {
            throw "The $channel package changed during acceptance."
        }
    }
    if (@($finalIdentity.checks | Where-Object { $_.status -ne 'pass' }).Count) {
        throw 'The candidate checksums changed during acceptance.'
    }
    $summary.passed = $records.Count -eq ($identity.checks.Count + $manual.Count) -and
        @($records | Where-Object { $_.status -ne 'pass' }).Count -eq 0
} catch {
    $summary.error = $_.Exception.Message
    Write-Host $summary.error -ForegroundColor Red
} finally {
    $summary.generated_at = (Get-Date).ToUniversalTime().ToString('o')
    $summaryPath = Join-Path $runDir 'acceptance-summary.json'
    $summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding utf8
    Write-Host "Wrote $summaryPath"
}
if (-not $summary.passed) { exit 1 }
