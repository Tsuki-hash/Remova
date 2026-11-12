# Validate the reviewed artifacts and four acceptance runs, or verify an
# approval against the actual draft attachments before they become public.
[CmdletBinding(DefaultParameterSetName = 'Verify')]
param(
    [Parameter(Mandatory)][ValidatePattern('^v\d+\.\d+\.\d+$')][string]$Tag,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$Commit,
    [Parameter(Mandatory)][string]$BundleDir,
    [string]$SevenZip,
    [string]$WixDark = $env:RELEASE_WIX_DARK,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string[]]$Acceptance,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string]$PackageExeReference,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string]$Startup,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string]$Components,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string]$ReleaseMetadataPath,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string]$PrivateKeyPath,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][switch]$Reviewed,
    [Parameter(Mandatory, ParameterSetName = 'Verify')][string]$ApprovalJson,
    [Parameter(Mandatory, ParameterSetName = 'Verify')][string]$PublicKeyPem
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

function ConvertTo-Sha256Hex([byte[]]$Hash) {
    [Convert]::ToHexString($Hash).ToLowerInvariant()
}

function Open-ReviewFile([string]$Path) {
    [IO.File]::Open((Resolve-Path -LiteralPath $Path).Path, 'Open', 'Read', 'Read')
}

function Get-PerformanceApproval([string]$ExeSha256) {
    $startupFile = Open-ReviewFile $Startup
    try {
        $componentsFile = Open-ReviewFile $Components
        try {
            & "$PSScriptRoot/acceptance/measure-components.ps1" -Startup $Startup -Components $Components `
                -CandidateCommit $Commit -CandidateExeSha256 $ExeSha256
            if ($LASTEXITCODE -ne 0) { throw 'The candidate did not pass the cold-start performance gate.' }
            [ordered]@{
                startup_sha256 = ConvertTo-Sha256Hex ([Security.Cryptography.SHA256]::HashData($startupFile))
                components_sha256 = ConvertTo-Sha256Hex ([Security.Cryptography.SHA256]::HashData($componentsFile))
                executable_sha256 = $ExeSha256
            }
        } finally { $componentsFile.Dispose() }
    } finally { $startupFile.Dispose() }
}

function Import-ApprovalKey($Rsa, [string]$Pem, [bool]$Private) {
    $expected = if ($Private) { '-----BEGIN (?:RSA )?PRIVATE KEY-----' } else { '-----BEGIN PUBLIC KEY-----' }
    if ($Pem -cnotmatch "^\s*$expected") {
        throw 'A release approval requires the configured RSA PEM key.'
    }
    $Rsa.ImportFromPem($Pem)
    if ($Rsa.KeySize -lt 2048) { throw 'A release approval requires an RSA key of at least 2048 bits.' }
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
function Get-ReviewedExecutables {
    if (!$SevenZip) {
        $SevenZip = (Get-Command 7z.exe -ErrorAction SilentlyContinue).Source
        if (!$SevenZip) { $SevenZip = Join-Path $env:ProgramFiles '7-Zip/7z.exe' }
    }
    if (!$WixDark) { $WixDark = Join-Path $env:LOCALAPPDATA 'tauri/WixTools314/dark.exe' }
    $extracted = & "$PSScriptRoot/acceptance/get-package-exe-reference.ps1" -Artifacts $BundleDir `
        -CandidateVersion $version -SevenZip $SevenZip -WixDark $WixDark -ExecutableOnly |
        ConvertFrom-Json -AsHashtable
    Assert-SameKeys $extracted.Keys $names.Keys 'Extracted package executables'
    foreach ($channel in $names.Keys) {
        if ($extracted[$channel].package_sha256 -cne $hashes[$names[$channel]] -or
            $extracted[$channel].executable_sha256 -cnotmatch '^[a-f0-9]{64}$') {
            throw "The $channel executable was not extracted from the reviewed attachment."
        }
    }
    $extracted
}
$sha256 = [Security.Cryptography.HashAlgorithmName]::SHA256
$pss = [Security.Cryptography.RSASignaturePadding]::Pss

if ($PSCmdlet.ParameterSetName -eq 'Prepare') {
    if (!$Reviewed) { throw 'Review the recorded observations, SBOM, release mode and recovery instructions first.' }
    $extracted = Get-ReviewedExecutables
    $exeHash = $extracted.portable.executable_sha256
    $packageIdentity = [ordered]@{}
    foreach ($channel in $names.Keys) { $packageIdentity[$channel] = @{ sha256 = $hashes[$names[$channel]] } }
    $executables = Read-AcceptanceExecutableReference $PackageExeReference $packageIdentity
    foreach ($channel in $names.Keys) {
        if ($executables[$channel] -cne $extracted[$channel].executable_sha256) {
            throw "The executable reference used a different $channel executable."
        }
    }
    $metadata = Get-Content -LiteralPath $ReleaseMetadataPath -Raw | ConvertFrom-Json -AsHashtable
    Assert-SameKeys $metadata.Keys @('name', 'body', 'prerelease', 'make_latest') 'Release metadata'
    if ($metadata.name -isnot [string] -or [string]::IsNullOrWhiteSpace($metadata.name) -or
        $metadata.body -isnot [string] -or $metadata.prerelease -isnot [bool] -or
        $metadata.make_latest -isnot [string] -or $metadata.make_latest -cnotin @('true', 'false') -or
        ($metadata.prerelease -and $metadata.make_latest -cne 'false')) {
        throw 'Invalid release title, notes, prerelease or latest setting.'
    }
    $publication = [ordered]@{
        name = $metadata.name
        body_sha256 = ConvertTo-Sha256Hex ([Security.Cryptography.SHA256]::HashData(
            [Text.Encoding]::UTF8.GetBytes($metadata.body)))
        prerelease = $metadata.prerelease
        make_latest = $metadata.make_latest
    }
    $verified = [ordered]@{}
    foreach ($file in $Acceptance) {
        $summaryBytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $file).Path)
        $summary = [Text.Encoding]::UTF8.GetString($summaryBytes).TrimStart([char]0xFEFF) | ConvertFrom-Json -AsHashtable
        $key = "$($summary.os_family)/$($summary.suite)"
        if ($key -cnotin $matrix -or $verified.Contains($key)) { throw "Unexpected or duplicate acceptance run for $key." }
        if ($summary.schema_version -ne 2 -or $summary.kind -cne 'remova-acceptance' -or
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
        Assert-SameKeys $summary.executables.Keys $names.Keys "Acceptance executables for $key"
        foreach ($channel in $names.Keys) {
            $name = $names[$channel]
            $observed = $summary.artifacts[$channel]
            if ($observed.name -cne $name -or $observed.sha256 -cne $hashes[$name]) {
                throw "Acceptance used a different $channel package for $key."
            }
            $launched = $summary.executables[$channel]
            if ($launched -isnot [Collections.IDictionary] -or $launched.Count -ne 5 -or
                $launched.path -isnot [string] -or -not (Test-AcceptanceExecutablePath $launched.path) -or
                $launched.version -isnot [string] -or -not (Test-AcceptanceExecutableVersion $launched.version $version) -or
                $launched.sha256 -isnot [string] -or $launched.sha256 -cne $executables[$channel] -or
                ($launched.pid -isnot [int] -and $launched.pid -isnot [long]) -or $launched.pid -le 0 -or
                $launched.uia_ready -isnot [bool] -or -not $launched.uia_ready) {
                throw "Invalid $channel executable identity or UI readiness for $key."
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
        $verified[$key] = ConvertTo-Sha256Hex ([Security.Cryptography.SHA256]::HashData($summaryBytes))
    }
    Assert-SameKeys $verified.Keys $matrix 'Acceptance matrix'
    $performance = Get-PerformanceApproval $exeHash
    $approval = [ordered]@{
        schema_version = 4
        tag = $Tag
        commit = $Commit
        assets = $hashes
        acceptance = $verified
        executables = $executables
        performance = $performance
        publication = $publication
    } | ConvertTo-Json -Depth 5 -Compress
    $bytes = [Text.Encoding]::UTF8.GetBytes($approval)
    $rsa = [Security.Cryptography.RSA]::Create()
    try {
        Import-ApprovalKey $rsa ([IO.File]::ReadAllText((Resolve-Path -LiteralPath $PrivateKeyPath).Path)) $true
        [ordered]@{
            payload = [Convert]::ToBase64String($bytes)
            signature = [Convert]::ToBase64String($rsa.SignData($bytes, $sha256, $pss))
        } | ConvertTo-Json -Compress
    } finally { $rsa.Dispose() }
    return
}

$envelope = $ApprovalJson | ConvertFrom-Json -AsHashtable
if (!$envelope.payload -or !$envelope.signature) { throw 'A signed release approval is required.' }
Assert-SameKeys $envelope.Keys @('payload', 'signature') 'Signed approval envelope'
$bytes = [Convert]::FromBase64String($envelope.payload)
$signature = [Convert]::FromBase64String($envelope.signature)
$rsa = [Security.Cryptography.RSA]::Create()
try {
    Import-ApprovalKey $rsa $PublicKeyPem $false
    if (!$rsa.VerifyData($bytes, $signature, $sha256, $pss)) { throw 'Invalid release approval signature.' }
} finally { $rsa.Dispose() }
$approval = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json -AsHashtable
Assert-SameKeys $approval.Keys @('schema_version', 'tag', 'commit', 'assets', 'acceptance', 'executables', 'performance', 'publication') 'Signed approval'
if ($approval.schema_version -ne 4 -or $approval.tag -cne $Tag -or $approval.commit -cne $Commit) {
    throw 'Approval belongs to a different release or commit.'
}
Assert-SameKeys $approval.acceptance.Keys $matrix 'Approved acceptance matrix'
foreach ($hash in $approval.acceptance.Values) {
    if ($hash -cnotmatch '^[a-f0-9]{64}$' -or $hash -cmatch '^0+$') { throw 'Invalid acceptance record digest.' }
}
Assert-SameKeys $approval.executables.Keys $names.Keys 'Approved package executables'
foreach ($hash in $approval.executables.Values) {
    if ($hash -isnot [string] -or $hash -cnotmatch '^[a-f0-9]{64}$' -or $hash -cmatch '^0+$') {
        throw 'Invalid approved package executable digest.'
    }
}
Assert-SameKeys $approval.assets.Keys $hashes.Keys 'Approved attachments'
foreach ($name in $hashes.Keys) {
    if ($approval.assets[$name] -cne $hashes[$name]) { throw "The draft attachment changed: $name." }
}
$extracted = Get-ReviewedExecutables
$exeHash = $extracted.portable.executable_sha256
foreach ($channel in $names.Keys) {
    if ($approval.executables[$channel] -cne $extracted[$channel].executable_sha256) {
        throw "The approval used a different $channel executable."
    }
}
Assert-SameKeys $approval.performance.Keys @('startup_sha256', 'components_sha256', 'executable_sha256') 'Approved performance evidence'
foreach ($key in @('startup_sha256', 'components_sha256')) {
    if ($approval.performance[$key] -cnotmatch '^[a-f0-9]{64}$' -or $approval.performance[$key] -cmatch '^0+$') {
        throw 'Invalid performance record digest.'
    }
}
if ($approval.performance.executable_sha256 -cne $exeHash) {
    throw 'The approved performance evidence used a different portable executable.'
}
Assert-SameKeys $approval.publication.Keys @('name', 'body_sha256', 'prerelease', 'make_latest') 'Approved release metadata'
if ($approval.publication.name -isnot [string] -or [string]::IsNullOrWhiteSpace($approval.publication.name) -or
    $approval.publication.body_sha256 -cnotmatch '^[a-f0-9]{64}$' -or
    $approval.publication.prerelease -isnot [bool] -or
    $approval.publication.make_latest -isnot [string] -or $approval.publication.make_latest -cnotin @('true', 'false') -or
    ($approval.publication.prerelease -and $approval.publication.make_latest -cne 'false')) {
    throw 'Invalid approved release metadata.'
}
$approval.publication
