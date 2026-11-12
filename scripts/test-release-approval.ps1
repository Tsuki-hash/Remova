$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance/acceptance-common.ps1"
$temp = (Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path
$root = Join-Path $temp "remova-release-approval-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $root | Out-Null
$approvalScripts = Join-Path $root 'scripts'
$releaseMetadata = @{ name = 'Remova 1.4.1'; body = "Release notes`nLine two"; prerelease = $false; make_latest = 'true' }

function Must-Refuse([scriptblock]$Action, [string]$Reason) {
    try { & $Action | Out-Null } catch {
        if ($_.Exception.Message -like "*$Reason*") { return }
        throw
    }
    throw "Expected refusal: $Reason"
}

function Test-LaunchObservation([string]$FixtureRoot) {
    if (-not $IsWindows) { return }
    $exe = Join-Path $FixtureRoot 'Remova.exe'
    Copy-Item -LiteralPath ([Diagnostics.Process]::GetCurrentProcess().MainModule.FileName) -Destination $exe
    $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($exe).ProductVersion
    $digest = (Get-FileHash -LiteralPath $exe).Hash.ToLowerInvariant()
    $state = @{ actualPath = $exe; ready = $true; running = $false; starts = 0; closed = 0 }
    function Get-Process { if ($state.running) { @{ Id = 17 } } }
    function Start-Process([string]$FilePath, [switch]$PassThru) {
        if (!$PassThru -or $FilePath -ine $exe) { throw 'Unexpected launch request' }
        $state.starts++
        @{ Id = 4321; HasExited = $false; MainModule = @{ FileName = $state.actualPath } }
    }
    function Wait-RemovaUiReady($Process) { $Process.Id -eq 4321 -and $state.ready }
    function Close-RemovaTestProcess($Process) { if ($Process.Id -eq 4321) { $state.closed++ } }

    $observed = Get-AcceptanceLaunchObservation $exe $version $digest
    if ($observed.path -ine $exe -or $observed.version -cne $version -or $observed.sha256 -cne $digest -or
        $observed.pid -ne 4321 -or !$observed.uia_ready -or $state.starts -ne 1 -or $state.closed -ne 1) {
        throw 'The launch observation did not record the verified executable and readiness.'
    }
    Must-Refuse { Get-AcceptanceLaunchObservation $exe $version ('f' * 64) } 'does not match the reviewed package'
    $state.running = $true
    Must-Refuse { Get-AcceptanceLaunchObservation $exe $version $digest } 'Close existing Remova processes'
    if ($state.starts -ne 1) { throw 'A stale or already running executable was launched.' }
    $state.running = $false
    $state.actualPath = Join-Path $FixtureRoot 'other\Remova.exe'
    Must-Refuse { Get-AcceptanceLaunchObservation $exe $version $digest } 'running process did not match'
    $state.ready = $false
    Must-Refuse { Get-AcceptanceLaunchObservation $exe $version $digest } 'did not show the enabled Software navigation button'
    if ($state.closed -ne $state.starts) { throw 'An unsuccessful launch was not closed.' }
}

function Test-PublishDraft(
    [string]$Scenario,
    [string]$FixtureRole,
    [string]$FixtureCommit,
    [string]$FixtureBundle,
    [string]$FixtureApproval,
    [string]$FixturePublicKey,
    [string]$FixtureRoot
) {
    $fixtureTag = 'v1.4.1'
    $fixtureRepository = 'fixture-org/fixture-remova'
    $fixtureActor = 'fixture-maintainer'
    $fixtureRelease = 700
    $state = @{ permissions = 0; commits = 0; drafts = 0; downloads = 0; patches = 0 }
    $assetId = 1000
    $files = @(Get-ChildItem -LiteralPath $FixtureBundle -File)
    $assets = @(
        foreach ($file in $files) {
            $assetId++
            @{
                id = $assetId
                name = $file.Name
                digest = 'sha256:' + (Get-FileHash -LiteralPath $file.FullName).Hash.ToLowerInvariant()
                state = 'uploaded'
            }
        }
    )
    $fixtureTemp = Join-Path $FixtureRoot "publish-$Scenario"
    New-Item -ItemType Directory -Path $fixtureTemp | Out-Null

    function gh {
        $global:LASTEXITCODE = 0
        switch -Exact ($args -join '|') {
            "api|repos/$fixtureRepository/collaborators/$fixtureActor/permission|--jq|.role_name" {
                $state.permissions++
                return $FixtureRole
            }
            "api|repos/$fixtureRepository/commits/$fixtureTag|--jq|.sha" {
                $state.commits++
                if ($Scenario -eq 'tag-changed' -and $state.commits -eq 2) { return ('b' * 40) }
                return $FixtureCommit
            }
            "api|repos/$fixtureRepository/releases?per_page=100|--paginate|--slurp" {
                $state.drafts++
                $draft = @{ id = $fixtureRelease; draft = $true; tag_name = $fixtureTag; assets = $assets
                    name = $releaseMetadata.name; body = $releaseMetadata.body; prerelease = $releaseMetadata.prerelease }
                if ($Scenario -eq 'wrong-approved-title') { $draft.name += ' changed' }
                if ($state.drafts -eq 2) {
                    if ($Scenario -eq 'asset-changed') { $assets[0].id += 100 }
                    if ($Scenario -eq 'draft-replaced') { $draft.id++ }
                    if ($Scenario -eq 'title-changed') { $draft.name += ' changed' }
                    if ($Scenario -eq 'body-changed') { $draft.body += ' changed' }
                    if ($Scenario -eq 'prerelease-changed') { $draft.prerelease = $true }
                }
                $earlier = @{ id = 42; draft = $false; tag_name = 'v1.4.0'; assets = @() }
                return ConvertTo-Json -InputObject @(@($earlier), @($draft)) -Depth 7 -Compress
            }
            "api|--method|PATCH|repos/$fixtureRepository/releases/$fixtureRelease|--input|$(Join-Path $fixtureTemp "reviewed-release-$fixtureRelease/publish-request.json")|--silent" {
                if ($state.commits -ne 2 -or $state.drafts -ne 2 -or $state.downloads -ne 1) {
                    throw 'The release was patched before verification completed.'
                }
                $request = Get-Content -LiteralPath $args[5] -Raw | ConvertFrom-Json -AsHashtable
                if ($request.draft -ne $false -or $request.name -cne $releaseMetadata.name -or
                    $request.body -cne $releaseMetadata.body -or $request.prerelease -ne $releaseMetadata.prerelease -or
                    $request.make_latest -cne $releaseMetadata.make_latest) {
                    throw 'Publication did not send the approved metadata.'
                }
                $state.patches++
                return
            }
        }
        $destination = Join-Path $fixtureTemp "reviewed-release-$fixtureRelease"
        $downloadArgs = "release|download|$fixtureTag|--repo|$fixtureRepository|--pattern|*|--dir|$destination"
        if (($args -join '|') -cne $downloadArgs) {
            throw "Unexpected fixture gh arguments: $($args -join ' ')"
        }
        $state.downloads++
        foreach ($file in $files) { Copy-Item -LiteralPath $file.FullName -Destination $destination }
    }

    # Force every gh call through the function above and clear any real
    # credentials for the duration of the fixture.
    $overrides = @{
        PATH = $fixtureTemp
        GH_TOKEN = $null
        GITHUB_TOKEN = $null
        GH_ENTERPRISE_TOKEN = $null
        GITHUB_ENTERPRISE_TOKEN = $null
        GH_PROMPT_DISABLED = '1'
        RELEASE_TAG = $fixtureTag
        GITHUB_REPOSITORY = $fixtureRepository
        DISPATCH_ACTOR = $fixtureActor
        RUNNER_TEMP = $fixtureTemp
        APPROVAL_JSON = $FixtureApproval
        RELEASE_APPROVAL_PUBLIC_KEY = if ($Scenario -ceq 'no-key') { $null } else { $FixturePublicKey }
    }
    $original = @{}
    try {
        foreach ($key in $overrides.Keys) {
            $original[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
            [Environment]::SetEnvironmentVariable($key, $overrides[$key], 'Process')
        }
        $publish = "$approvalScripts/publish-release-draft.ps1"
        switch ($Scenario) {
            'success' { & $publish 6>&1 | Out-Null }
            'unauthorized' { Must-Refuse { & $publish } 'requires a repository maintainer or administrator' }
            'no-key' { Must-Refuse { & $publish } 'Configure the RELEASE_APPROVAL_PUBLIC_KEY' }
            'unsigned' { Must-Refuse { & $publish } 'signed release approval is required' }
            'wrong-approved-title' { Must-Refuse { & $publish } 'metadata does not match the signed approval' }
            default { Must-Refuse { & $publish } 'changed during verification' }
        }
    } finally {
        foreach ($key in $original.Keys) {
            [Environment]::SetEnvironmentVariable($key, $original[$key], 'Process')
        }
    }

    $expected = switch ($Scenario) {
        'success' { '1/2/2/1/1' }
        'unauthorized' { '1/0/0/0/0' }
        'no-key' { '0/0/0/0/0' }
        'unsigned' { '1/1/1/1/0' }
        'wrong-approved-title' { '1/1/1/1/0' }
        'tag-changed' { '1/2/2/1/0' }
        default { '1/1/2/1/0' }
    }
    $calls = "$($state.permissions)/$($state.commits)/$($state.drafts)/$($state.downloads)/$($state.patches)"
    if ($calls -cne $expected) { throw "Unexpected $Scenario fixture calls: $calls, expected $expected." }
}

try {
    # These small attachments contain fixture bytes. Replace only the
    # extraction adapter; the real approval and publication logic runs below.
    $acceptanceScripts = Join-Path $approvalScripts 'acceptance'
    New-Item -ItemType Directory -Path $acceptanceScripts -Force | Out-Null
    Copy-Item -LiteralPath @("$PSScriptRoot/release-approval.ps1", "$PSScriptRoot/publish-release-draft.ps1") -Destination $approvalScripts
    Copy-Item -LiteralPath "$PSScriptRoot/acceptance/acceptance-common.ps1" -Destination $acceptanceScripts
    @'
param($Artifacts, $CandidateVersion, $SevenZip, $WixDark, [switch]$ExecutableOnly)
. "$PSScriptRoot/acceptance-common.ps1"
if (!$ExecutableOnly) { throw 'Unexpected extraction mode.' }
$identity = Get-AcceptanceArtifacts $Artifacts $CandidateVersion
$reference = [ordered]@{}
foreach ($channel in $identity.artifacts.Keys) {
    $asset = $identity.artifacts[$channel]
    $path = Join-Path $Artifacts $asset.name
    $exe = if ($channel -ceq 'portable') { Get-PortableExeHash $path $asset.sha256 }
        else { (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() }
    $reference[$channel] = @{ package_sha256 = $asset.sha256; executable_sha256 = $exe }
}
$reference | ConvertTo-Json -Depth 3
'@ | Set-Content -LiteralPath (Join-Path $acceptanceScripts 'get-package-exe-reference.ps1')

    $launchFixture = New-Item -ItemType Directory -Path (Join-Path $root 'launch')
    Test-LaunchObservation $launchFixture.FullName
    $tag = 'v1.4.1'
    $commit = 'a' * 40
    $bundle = New-Item -ItemType Directory -Path (Join-Path $root 'bundle')
    $rsa = [Security.Cryptography.RSA]::Create(2048)
    $privateKey = Join-Path $root 'fixture-private.pem'
    [IO.File]::WriteAllText($privateKey, $rsa.ExportPkcs8PrivateKeyPem())
    $publicKey = $rsa.ExportSubjectPublicKeyInfoPem()
    $portableExe = Join-Path $root 'Remova.exe'
    Set-Content -LiteralPath $portableExe -Value 'fixture portable executable'
    $portableExeHash = (Get-FileHash -LiteralPath $portableExe).Hash.ToLowerInvariant()
    $names = [ordered]@{
        nsis = 'Remova_1.4.1_x64-setup.exe'
        msi = 'Remova_1.4.1_x64_zh-CN.msi'
        portable = 'Remova_1.4.1_x64-portable.zip'
    }
    $artifacts = [ordered]@{}
    $sums = foreach ($channel in $names.Keys) {
        $name = $names[$channel]
        $file = Join-Path $bundle.FullName $name
        if ($channel -ceq 'portable') { Compress-Archive -LiteralPath $portableExe -DestinationPath $file }
        else { Set-Content -LiteralPath $file -Value "fixture $channel" }
        $hash = (Get-FileHash -LiteralPath $file).Hash.ToLowerInvariant()
        $artifacts[$channel] = @{ name = $name; sha256 = $hash }
        "$hash  $name"
    }
    Set-Content -LiteralPath (Join-Path $bundle.FullName 'SHA256SUMS.txt') -Value $sums
    Set-Content -LiteralPath (Join-Path $bundle.FullName 'SBOM.cdx.json') -Value '{"bomFormat":"CycloneDX","metadata":{"component":{"version":"1.4.1"}},"components":[{"name":"fixture"}]}'
    $exeHashes = [ordered]@{ nsis = $artifacts.nsis.sha256; msi = $artifacts.msi.sha256; portable = $portableExeHash }
    $reference = [ordered]@{}
    foreach ($channel in $names.Keys) {
        $reference[$channel] = @{ package_sha256 = $artifacts[$channel].sha256; executable_sha256 = $exeHashes[$channel] }
    }
    $referencePath = Join-Path $root 'package-exe-reference.json'
    $reference | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $referencePath

    $evidence = foreach ($mode in @('client', 'elevated')) {
        $record = @{
            schema_version = 1
            kind = 'remova-release-launch'
            candidate_commit = $commit
            candidate_version = '1.4.1'
            environment = @{
                machine_sha256 = 'd' * 64
                os_version = '10.0.26200'
                architecture = 'x64'
                webview2_version = '1.2.3.4'
                elevated = $mode -eq 'elevated'
            }
            executable = @{ path = 'C:\fixture\remova.exe'; version = '1.4.1';
                sha256 = $portableExeHash; pid = 4321; uia_ready = $true }
        }
        $file = Join-Path $root "$mode-launch.json"
        $record | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $file
        $file
    }
    $check = "$approvalScripts/release-approval.ps1"
    $params = @{ Tag = $tag; Commit = $commit; BundleDir = $bundle.FullName }
    $metadataPath = Join-Path $root 'release-metadata.json'
    $releaseMetadata | ConvertTo-Json | Set-Content -LiteralPath $metadataPath
    $prepare = @{ ReleaseMetadataPath = $metadataPath
        PackageExeReference = $referencePath; PrivateKeyPath = $privateKey; Reviewed = $true }
    $approval = & $check @params @prepare -LaunchRecord $evidence
    $null = & $check @params -ApprovalJson $approval -PublicKeyPem $publicKey
    Must-Refuse { & $check @params @prepare -LaunchRecord $evidence[0] } 'Local launch records'
    Must-Refuse { & $check @params @prepare -LaunchRecord @($evidence[0], $evidence[0]) } 'Duplicate local launch record'
    Must-Refuse { & $check @params -ApprovalJson '{}' -PublicKeyPem $publicKey } 'signed release approval is required'
    $wrongCommit = $params.Clone()
    $wrongCommit.Commit = 'b' * 40
    Must-Refuse { & $check @wrongCommit -ApprovalJson $approval -PublicKeyPem $publicKey } 'different release or commit'
    $wrongRsa = [Security.Cryptography.RSA]::Create(2048)
    try {
        $wrongKey = $wrongRsa.ExportSubjectPublicKeyInfoPem()
        Must-Refuse { & $check @params -ApprovalJson $approval -PublicKeyPem $wrongKey } 'Invalid release approval signature'
    } finally { $wrongRsa.Dispose() }

    $envelope = $approval | ConvertFrom-Json -AsHashtable
    $signed = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($envelope.payload)) | ConvertFrom-Json -AsHashtable
    foreach ($channel in $names.Keys) {
        if ($signed.executables[$channel] -cne $exeHashes[$channel]) { throw "Approval did not bind the $channel executable." }
    }
    $originalSummary = Get-Content -Raw -LiteralPath $evidence[0]
    foreach ($scenario in @('old-build', 'other-channel', 'missing-ui', 'old-version', 'relative-path',
        'missing-executable', 'other-machine', 'other-commit', 'not-a-token')) {
        $observed = $originalSummary | ConvertFrom-Json -AsHashtable
        $reason = 'Invalid portable executable identity or UI readiness'
        switch ($scenario) {
            'old-build' { $observed.executable.sha256 = 'f' * 64 }
            'other-channel' { $observed.executable.sha256 = $exeHashes.msi }
            'missing-ui' { $observed.executable.uia_ready = $false }
            'old-version' { $observed.executable.version = '1.4.0' }
            'relative-path' { $observed.executable.path = 'remova.exe' }
            'missing-executable' { $observed.Remove('executable') }
            'other-machine' {
                $observed.environment.machine_sha256 = 'f' * 64
                $reason = 'same Windows machine'
            }
            'other-commit' {
                $observed.candidate_commit = 'b' * 40
                $reason = 'Invalid local launch record or candidate identity'
            }
            'not-a-token' {
                $observed.environment.elevated = 'false'
                $reason = 'Invalid local launch record or candidate identity'
            }
        }
        $observed | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $evidence[0]
        Must-Refuse { & $check @params @prepare -LaunchRecord $evidence } $reason
    }
    Set-Content -LiteralPath $evidence[0] -Value $originalSummary -NoNewline
    $reference.nsis.package_sha256 = 'f' * 64
    $reference | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $referencePath
    Must-Refuse { & $check @params @prepare -LaunchRecord $evidence } 'different nsis package'
    $reference.nsis.package_sha256 = $artifacts.nsis.sha256
    $reference.nsis.executable_sha256 = $exeHashes.msi
    $reference | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $referencePath
    Must-Refuse { & $check @params @prepare -LaunchRecord $evidence } 'reference used a different nsis executable'
    $reference.nsis.executable_sha256 = $exeHashes.nsis
    $reference | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $referencePath
    $signed.executables.msi = $exeHashes.portable
    $differentExecutable = [Text.Encoding]::UTF8.GetBytes(($signed | ConvertTo-Json -Depth 7 -Compress))
    $wrongExecutableApproval = @{
        payload = [Convert]::ToBase64String($differentExecutable)
        signature = [Convert]::ToBase64String($rsa.SignData($differentExecutable,
            [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pss))
    } | ConvertTo-Json -Compress
    Must-Refuse { & $check @params -ApprovalJson $wrongExecutableApproval -PublicKeyPem $publicKey } 'approval used a different msi executable'
    $signed.executables.msi = $exeHashes.msi
    $recordHashes = @(foreach ($file in $evidence) { (Get-FileHash -LiteralPath $file).Hash.ToLowerInvariant() })
    if ($signed.launches.client -cne $recordHashes[0] -or $signed.launches.elevated -cne $recordHashes[1]) {
        throw 'The approval did not bind both local launch records.'
    }
    $signed.launches.client = '0' * 64
    $modifiedBytes = [Text.Encoding]::UTF8.GetBytes(($signed | ConvertTo-Json -Depth 7 -Compress))
    $envelope.payload = [Convert]::ToBase64String($modifiedBytes)
    Must-Refuse { & $check @params -ApprovalJson ($envelope | ConvertTo-Json -Compress) -PublicKeyPem $publicKey } 'Invalid release approval signature'
    $envelope.signature = [Convert]::ToBase64String($rsa.SignData($modifiedBytes,
        [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pss))
    Must-Refuse { & $check @params -ApprovalJson ($envelope | ConvertTo-Json -Compress) -PublicKeyPem $publicKey } 'Invalid local launch record digest'

    Test-PublishDraft 'success' 'maintain' $commit $bundle.FullName $approval $publicKey $root
    Test-PublishDraft 'unauthorized' 'write' $commit $bundle.FullName $approval $publicKey $root
    Test-PublishDraft 'no-key' 'maintain' $commit $bundle.FullName $approval $publicKey $root
    Test-PublishDraft 'unsigned' 'maintain' $commit $bundle.FullName ($signed | ConvertTo-Json -Depth 7 -Compress) $publicKey $root
    foreach ($scenario in 'asset-changed', 'draft-replaced', 'tag-changed', 'wrong-approved-title',
        'title-changed', 'body-changed', 'prerelease-changed') {
        Test-PublishDraft $scenario 'admin' $commit $bundle.FullName $approval $publicKey $root
    }
    Write-Host 'Draft publication fixture passed the maintainer, permission, changed-asset, replaced-draft and changed-tag checks.'

    Add-Content -LiteralPath (Join-Path $bundle.FullName 'SBOM.cdx.json') -Value ' '
    Must-Refuse { & $check @params -ApprovalJson $approval -PublicKeyPem $publicKey } 'draft attachment changed'
    Write-Host 'Release approval passed the local launches, missing-run, wrong-approval, changed-asset and changed-record checks.'
} finally {
    if ($rsa) { $rsa.Dispose() }
    $resolved = (Resolve-Path -LiteralPath $root).Path
    if ((Split-Path -Parent $resolved) -eq $temp -and (Split-Path -Leaf $resolved) -match '^remova-release-approval-[a-f0-9]{32}$') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
