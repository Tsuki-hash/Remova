# Record an observed launch of the exact portable release executable.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ExePath,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$CandidateCommit,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$CandidateVersion,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedExeSha256,
    [Parameter(Mandatory)][ValidateSet('client','elevated')][string]$Mode,
    [Parameter(Mandatory)][string]$Out
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance-common.ps1"
. "$PSScriptRoot/performance-common.ps1"

if ((Test-AcceptanceElevated) -ne ($Mode -eq 'elevated')) {
    throw "The current Windows token does not match the $Mode launch."
}
if (Test-Path -LiteralPath $Out) { throw 'Choose a new path for the launch record.' }
$environment = Get-PerformanceEnvironment
$observation = Get-AcceptanceLaunchObservation $ExePath $CandidateVersion ($ExpectedExeSha256.ToLowerInvariant())
[ordered]@{
    schema_version = 1
    kind = 'remova-release-launch'
    candidate_commit = $CandidateCommit.ToLowerInvariant()
    candidate_version = $CandidateVersion
    environment = $environment
    executable = $observation
    observed_at = [DateTimeOffset]::UtcNow.ToString('o')
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Out -Encoding utf8
Write-Host "Observed $Mode WebView2 readiness and wrote $Out."
