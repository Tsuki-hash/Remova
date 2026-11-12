# Validate the reviewed artifacts and both local launch records, or verify an
# approval against the actual draft attachments before they become public.
[CmdletBinding(DefaultParameterSetName = 'Verify')]
param(
    [Parameter(Mandatory)][ValidatePattern('^v\d+\.\d+\.\d+$')][string]$Tag,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$Commit,
    [Parameter(Mandatory)][string]$BundleDir,
    [string]$SevenZip,
    [string]$WixDark = $env:RELEASE_WIX_DARK,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string[]]$LaunchRecord,
    [Parameter(Mandatory, ParameterSetName = 'Prepare')][string]$PackageExeReference,
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
$launchModes = @('client', 'elevated')

function Assert-SameKeys($Actual, $Expected, [string]$Context) {
    if (@($Actual).Count -ne @($Expected).Count -or (Compare-Object @($Actual) @($Expected) -CaseSensitive)) {
        throw "$Context does not contain exactly the expected entries."
    }
}

function ConvertTo-Sha256Hex([byte[]]$Hash) {
    [Convert]::ToHexString($Hash).ToLowerInvariant()
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
    $machine = $null
    foreach ($file in $LaunchRecord) {
        $recordBytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $file).Path)
        $record = [Text.Encoding]::UTF8.GetString($recordBytes).TrimStart([char]0xFEFF) | ConvertFrom-Json -AsHashtable
        $environment = $record.environment
        $launch = $record.executable
        if ($record.schema_version -ne 1 -or $record.kind -cne 'remova-release-launch' -or
            $record.candidate_commit -cne $Commit -or $record.candidate_version -cne $version -or
            $environment.elevated -isnot [bool] -or $environment.architecture -cne 'x64' -or
            $environment.machine_sha256 -cnotmatch '^[a-f0-9]{64}$' -or
            $environment.os_version -notmatch '^10\.0\.\d+(?:\.\d+)?$' -or
            $environment.webview2_version -notmatch '^[1-9]\d*\.\d+\.\d+\.\d+$') {
            throw 'Invalid local launch record or candidate identity.'
        }
        $mode = if ($environment.elevated) { 'elevated' } else { 'client' }
        if ($verified.Contains($mode)) { throw "Duplicate local launch record for $mode." }
        if ($machine -and $machine -cne $environment.machine_sha256) {
            throw 'Both local launches must use the same Windows machine.'
        }
        $machine = $environment.machine_sha256
        if ($launch -isnot [Collections.IDictionary] -or $launch.Count -ne 5 -or
            $launch.path -isnot [string] -or !(Test-AcceptanceExecutablePath $launch.path) -or
            $launch.version -isnot [string] -or !(Test-AcceptanceExecutableVersion $launch.version $version) -or
            $launch.sha256 -cne $executables.portable -or
            ($launch.pid -isnot [int] -and $launch.pid -isnot [long]) -or $launch.pid -le 0 -or
            $launch.uia_ready -isnot [bool] -or !$launch.uia_ready) {
            throw "Invalid portable executable identity or UI readiness for $mode."
        }
        $verified[$mode] = ConvertTo-Sha256Hex ([Security.Cryptography.SHA256]::HashData($recordBytes))
    }
    Assert-SameKeys $verified.Keys $launchModes 'Local launch records'
    $approval = [ordered]@{
        schema_version = 5
        tag = $Tag
        commit = $Commit
        assets = $hashes
        launches = $verified
        executables = $executables
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
Assert-SameKeys $approval.Keys @('schema_version', 'tag', 'commit', 'assets', 'launches', 'executables', 'publication') 'Signed approval'
if ($approval.schema_version -ne 5 -or $approval.tag -cne $Tag -or $approval.commit -cne $Commit) {
    throw 'Approval belongs to a different release or commit.'
}
Assert-SameKeys $approval.launches.Keys $launchModes 'Approved local launches'
foreach ($hash in $approval.launches.Values) {
    if ($hash -isnot [string] -or $hash -cnotmatch '^[a-f0-9]{64}$' -or $hash -cmatch '^0+$') {
        throw 'Invalid local launch record digest.'
    }
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
foreach ($channel in $names.Keys) {
    if ($approval.executables[$channel] -cne $extracted[$channel].executable_sha256) {
        throw "The approval used a different $channel executable."
    }
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
