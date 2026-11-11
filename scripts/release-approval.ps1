# Validate the reviewed artifacts and four acceptance runs, or verify an
# approval against the actual draft attachments before they become public.
[CmdletBinding(DefaultParameterSetName = 'Verify')]
param(
    [Parameter(Mandatory)][ValidatePattern('^v\d+\.\d+\.\d+$')][string]$Tag,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$Commit,
    [Parameter(Mandatory)][string]$BundleDir,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string[]]$Acceptance,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][switch]$Reviewed,
    [Parameter(Mandatory, ParameterSetName = 'Verify')][string]$ApprovalJson
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance/acceptance-common.ps1"
$version = $Tag.Substring(1)
$names = [ordered]@{
    nsis = "Remova_${version}_x64-setup.exe"
    msi = "Remova_${version}_x64_zh-CN.msi"
    portable = "Remova_${version}_x64-portable.zip"
}
$requiredAssets = @($names.Values) + @('SHA256SUMS.txt', 'SBOM.cdx.json')
$updaterAssets = @('latest.json', "$($names.nsis).sig", "$($names.msi).sig")
$matrix = @('windows-10/client', 'windows-10/elevated', 'windows-11/client', 'windows-11/elevated')

function Assert-SameKeys($Actual, $Expected, [string]$Context) {
    if (@($Actual).Count -ne @($Expected).Count -or (Compare-Object @($Actual) @($Expected) -CaseSensitive)) {
        throw "$Context does not contain exactly the expected entries."
    }
}

$hashes = [ordered]@{}
foreach ($asset in Get-ChildItem -LiteralPath $BundleDir -File | Sort-Object Name) {
    $hashes[$asset.Name] = (Get-FileHash -LiteralPath $asset.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
}
$includesUpdater = @($updaterAssets | Where-Object { $hashes.Contains($_) }).Count -gt 0
$expectedAssets = if ($includesUpdater) { $requiredAssets + $updaterAssets } else { $requiredAssets }
Assert-SameKeys $hashes.Keys $expectedAssets 'Release attachments'

$sums = @{}
foreach ($line in Get-Content -LiteralPath (Join-Path $BundleDir 'SHA256SUMS.txt')) {
    if ($line -notmatch '^([a-fA-F0-9]{64})\s+\*?([^/\\]+)$' -or $sums.ContainsKey($Matches[2])) {
        throw 'Invalid or duplicate package checksum.'
    }
    $sums[$Matches[2]] = $Matches[1].ToLowerInvariant()
}
Assert-SameKeys $sums.Keys $names.Values 'Package checksum file'
foreach ($name in $names.Values) {
    if ($sums[$name] -cne $hashes[$name]) { throw "Package checksum mismatch for $name." }
}
$sbom = Get-Content -LiteralPath (Join-Path $BundleDir 'SBOM.cdx.json') -Raw | ConvertFrom-Json -AsHashtable
if ($sbom.bomFormat -cne 'CycloneDX' -or $sbom.metadata.component.version -cne $version -or !$sbom.components.Count) {
    throw 'SBOM does not identify the candidate version and its components.'
}

if ($PSCmdlet.ParameterSetName -eq 'Prepare') {
    if (!$Reviewed) { throw 'Review the recorded observations, SBOM, release mode and recovery instructions first.' }
    $verified = [ordered]@{}
    foreach ($file in $Acceptance) {
        $summary = Get-Content -LiteralPath $file -Raw | ConvertFrom-Json -AsHashtable
        $key = "$($summary.os_family)/$($summary.suite)"
        if ($key -cnotin $matrix -or $verified.Contains($key)) { throw "Unexpected or duplicate acceptance run for $key." }
        if ($summary.schema_version -ne 1 -or $summary.kind -cne 'remova-acceptance' -or
            $summary.passed -isnot [bool] -or !$summary.passed -or
            $summary.candidate_commit -cne $Commit -or $summary.candidate_version -cne $version -or
            $summary.architecture -cne 'x64' -or $summary.elevated -isnot [bool] -or
            $summary.elevated -ne ($summary.suite -ceq 'elevated') -or
            $summary.os_version -notmatch '^10\.0\.(\d+)(?:\.\d+)?$' -or
            (($summary.os_family -ceq 'windows-11') -ne ([int]$Matches[1] -ge 22000)) -or
            [string]::IsNullOrWhiteSpace($summary.webview2_version) -or
            [string]::IsNullOrWhiteSpace($summary.executor)) {
            throw "Invalid acceptance result for $key."
        }
        Assert-SameKeys $summary.artifacts.Keys $names.Keys "Acceptance artifacts for $key"
        foreach ($channel in $names.Keys) {
            $name = $names[$channel]
            $observed = $summary.artifacts[$channel]
            if ($observed.name -cne $name -or $observed.sha256 -cne $hashes[$name]) {
                throw "Acceptance used a different $channel package for $key."
            }
        }
        $checks = @{}
        foreach ($check in $summary.checks) {
            if ($checks.ContainsKey($check.id)) { throw "Duplicate acceptance check in $key." }
            $checks[$check.id] = $check
        }
        $required = @(Get-AcceptanceChecks $summary.suite)
        Assert-SameKeys $checks.Keys @($required.id) "Acceptance checks for $key"
        foreach ($check in $required) {
            $result = $checks[$check.id]
            if ($result.status -cne 'pass' -or [string]::IsNullOrWhiteSpace($result.actual) -or
                ($check.requires_evidence -and
                    ([string]::IsNullOrWhiteSpace($result.evidence.name) -or
                     $result.evidence.sha256 -cnotmatch '^[a-f0-9]{64}$'))) {
                throw "Missing successful observation or evidence for $($check.id) in $key."
            }
            if ($check.requires_evidence) {
                $name = $result.evidence.name
                if ($name -cne [IO.Path]::GetFileName($name)) { throw 'Evidence must be stored beside its summary.' }
                $evidence = Get-Item -LiteralPath (Join-Path (Split-Path -Parent (Resolve-Path -LiteralPath $file).Path) $name)
                if ($evidence.PSIsContainer -or !$evidence.Length -or
                    (Get-FileHash -LiteralPath $evidence.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -cne $result.evidence.sha256) {
                    throw "Evidence changed for $($check.id) in $key."
                }
            }
        }
        $verified[$key] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    Assert-SameKeys $verified.Keys $matrix 'Acceptance matrix'
    [ordered]@{
        schema_version = 1
        tag = $Tag
        commit = $Commit
        assets = $hashes
        acceptance = $verified
    } | ConvertTo-Json -Depth 5 -Compress
    return
}

$approval = $ApprovalJson | ConvertFrom-Json -AsHashtable
if ($approval.schema_version -ne 1 -or $approval.tag -cne $Tag -or $approval.commit -cne $Commit) {
    throw 'Approval belongs to a different release or commit.'
}
Assert-SameKeys $approval.acceptance.Keys $matrix 'Approved acceptance matrix'
foreach ($hash in $approval.acceptance.Values) {
    if ($hash -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid acceptance record digest.' }
}
Assert-SameKeys $approval.assets.Keys $hashes.Keys 'Approved attachments'
foreach ($name in $hashes.Keys) {
    if ($approval.assets[$name] -cne $hashes[$name]) { throw "The draft attachment changed: $name." }
}
