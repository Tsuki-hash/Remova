$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance/acceptance-common.ps1"
. "$PSScriptRoot/release-notes-policy.ps1"
$temp = (Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path
$root = Join-Path $temp "remova-release-approval-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $root | Out-Null
$approvalScripts = Join-Path $root 'scripts'
$releaseBody = @'
## Remova 1.4.1

本版提供未签名安装包，更新请手动下载。Windows 可能显示发布者或 SmartScreen 提示。

### 下载与校验

从此发布页下载所选发行包和 `SHA256SUMS.txt`。在下载目录打开 PowerShell，将示例文件名替换为实际下载的 EXE、MSI 或 ZIP 文件名。

```powershell
Get-FileHash -LiteralPath '.\Remova_1.4.1_x64-setup.exe' -Algorithm SHA256
```

将输出的 Hash 与 `SHA256SUMS.txt` 中同名文件的摘要比对。一致后再运行安装程序或解压便携版。

### Changes

An example release.
'@
$releaseMetadata = @{ name = 'Remova 1.4.1'; body = $releaseBody; prerelease = $false; make_latest = 'true' }

# Only these fixtures use synthetic package bytes.
function Get-AuthenticodeSignature([string]$LiteralPath) {
    if (!(Test-Path -LiteralPath $LiteralPath -PathType Leaf)) { throw 'Missing fixture installer.' }
    return @{ Status = 'NotSigned' }
}

$ciFailures = @{
    'ci-wrong-sha' = 'does not match the release commit'
    'ci-pr' = 'does not match the release commit'
    'ci-not-on-main' = 'is not on main'
    'ci-no-run' = 'No main push CI run exists'
    'ci-api-error' = 'GitHub API call failed while checking release CI'
    'ci-pending' = "is still 'in_progress'"
    'ci-failed' = "concluded 'failure'"
    'ci-job-failed' = "CI job 'test' must have one successful result"
    'ci-job-skipped' = "CI job 'package' must have one successful result"
    'ci-job-missing' = "CI job 'package' must have one successful result"
    'ci-job-wrong-sha' = "CI job 'package' must have one successful result"
    'ci-job-duplicate' = "CI job 'package' must have one successful result"
    'ci-rerun' = 'CI changed during verification'
    'ci-late-failure' = "concluded 'failure'"
}

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

function Test-PortableSmoke([string]$FixtureRoot) {
    $scripts = Join-Path $FixtureRoot 'scripts'
    $acceptance = Join-Path $scripts 'acceptance'
    $release = Join-Path $FixtureRoot 'src-tauri/target/release'
    $portable = Join-Path $release 'bundle/portable'
    New-Item -ItemType Directory -Path $acceptance, $portable -Force | Out-Null
    Copy-Item -LiteralPath "$PSScriptRoot/smoke-portable.ps1" -Destination $scripts
    Copy-Item -LiteralPath "$PSScriptRoot/acceptance/acceptance-common.ps1" -Destination $acceptance
    Set-Content -LiteralPath (Join-Path $FixtureRoot 'package.json') -Value '{"version":"1.4.1"}'
    Set-Content -LiteralPath (Join-Path $FixtureRoot 'LICENSE') -Value 'fixture license'
    Set-Content -LiteralPath (Join-Path $FixtureRoot 'THIRD-PARTY-NOTICES.md') -Value 'fixture notices'
    $exe = Join-Path $release 'remova.exe'
    $original = 'fixture built exe bytes'
    Set-Content -LiteralPath $exe -Value $original -NoNewline
    $zip = Join-Path $portable 'Remova_1.4.1_x64-portable.zip'
    $smoke = Join-Path $scripts 'smoke-portable.ps1'

    function Write-FixtureZip([string[]]$Executables, [string]$Bytes) {
        $stream = [IO.File]::Open($zip, 'Create', 'Write', 'None')
        try {
            $archive = [IO.Compression.ZipArchive]::new($stream, 'Create', $true)
            try {
                foreach ($doc in @('LICENSE', 'THIRD-PARTY-NOTICES.md')) {
                    $null = [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, (Join-Path $FixtureRoot $doc), $doc)
                }
                $entries = @(@{ Name = 'PORTABLE.txt'; Text = 'Remova 1.4.1 requires WebView2' })
                $entries += @($Executables | ForEach-Object { @{ Name = $_; Text = $Bytes } })
                foreach ($entry in $entries) {
                    $writer = [IO.StreamWriter]::new($archive.CreateEntry($entry.Name).Open())
                    try { $writer.Write($entry.Text) } finally { $writer.Dispose() }
                }
            } finally { $archive.Dispose() }
        } finally { $stream.Dispose() }
    }

    $saved = $env:REMOVA_SMOKED_EXECUTABLE_SHA256
    try {
        $env:REMOVA_SMOKED_EXECUTABLE_SHA256 = (Get-FileHash -LiteralPath $exe).Hash.ToLowerInvariant()
        Write-FixtureZip @('Remova.exe') $original
        & $smoke
        Write-FixtureZip @('Remova.exe') 'different bytes pretending to be the exe'
        Must-Refuse { & $smoke } 'archived Remova.exe differs from the built executable'
        foreach ($duplicate in @('remova.exe', 'nested/Remova.exe')) {
            Write-FixtureZip @('Remova.exe', $duplicate) $original
            Must-Refuse { & $smoke } 'exactly one nonempty Remova.exe'
        }
        Write-FixtureZip @('nested/Remova.exe') $original
        Must-Refuse { & $smoke } 'exactly one nonempty Remova.exe'
        Set-Content -LiteralPath $exe -Value 'a different build' -NoNewline
        Write-FixtureZip @('Remova.exe') 'a different build'
        Must-Refuse { & $smoke } 'differs from the executable used for launch smoke'
    } finally { $env:REMOVA_SMOKED_EXECUTABLE_SHA256 = $saved }
    Write-Host 'Portable smoke passed the executable digest, duplicate entry and launch identity checks.'
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
    $state = @{ permissions = 0; commits = 0; drafts = 0; downloads = 0; patches = 0; compares = 0; runs = 0; jobs = 0; publicReads = 0 }
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
            "api|repos/$fixtureRepository/commits/tags/$fixtureTag|--jq|.sha" {
                $state.commits++
                if ($Scenario -eq 'tag-changed' -and $state.commits -eq 2) { return ('b' * 40) }
                if ($Scenario -eq 'public-tag-changed' -and $state.commits -eq 3) { return ('b' * 40) }
                return $FixtureCommit
            }
            "api|repos/$fixtureRepository/compare/main...$FixtureCommit|--jq|.status" {
                $state.compares++
                if ($Scenario -eq 'ci-not-on-main') { return 'ahead' }
                return 'behind'
            }
            "api|repos/$fixtureRepository/actions/workflows/ci.yml/runs?head_sha=$FixtureCommit&branch=main&event=push&per_page=100|--paginate|--slurp" {
                $state.runs++
                if ($Scenario -eq 'ci-api-error') {
                    $global:LASTEXITCODE = 1
                    return
                }
                if ($Scenario -eq 'ci-no-run') { return '[{"workflow_runs":[]}]' }
                $run = @{ id = 12; run_number = 14; run_attempt = 2; head_sha = $FixtureCommit
                    head_branch = 'main'; event = 'push'; status = 'completed'; conclusion = 'success' }
                switch ($Scenario) {
                    'ci-wrong-sha' { $run.head_sha = 'b' * 40 }
                    'ci-pr' { $run.event = 'pull_request' }
                    'ci-pending' { $run.status = 'in_progress'; $run.conclusion = $null }
                    'ci-failed' { $run.conclusion = 'failure' }
                    'ci-rerun' { if ($state.runs -eq 2) { $run.run_attempt++ } }
                    'ci-late-failure' { if ($state.runs -eq 3) { $run.conclusion = 'failure' } }
                }
                $older = $run.Clone()
                $older.run_number = 13
                $older.id = 11
                $older.conclusion = 'success'
                return ConvertTo-Json -InputObject @(@{workflow_runs = @($older, $run)}) -Depth 7 -Compress
            }
            "api|repos/$fixtureRepository/actions/runs/12/jobs?filter=latest&per_page=100|--paginate|--slurp" {
                $state.jobs++
                $test = @{ run_id = 12; head_sha = $FixtureCommit; name = 'test'; status = 'completed'; conclusion = 'success' }
                $package = @{ run_id = 12; head_sha = $FixtureCommit; name = 'package'; status = 'completed'; conclusion = 'success' }
                if ($Scenario -eq 'ci-job-failed') { $test.conclusion = 'failure' }
                if ($Scenario -eq 'ci-job-skipped') { $package.conclusion = 'skipped' }
                if ($Scenario -eq 'ci-job-wrong-sha') { $package.head_sha = 'b' * 40 }
                $all = if ($Scenario -eq 'ci-job-missing') { @($test) } else { @($test, $package) }
                if ($Scenario -eq 'ci-job-duplicate') { $all += $package.Clone() }
                return ConvertTo-Json -InputObject @(@{jobs = @($all)}) -Depth 7 -Compress
            }
            "api|repos/$fixtureRepository/releases?per_page=100|--paginate|--slurp" {
                $state.drafts++
                $draft = @{ id = $fixtureRelease; draft = $true; tag_name = $fixtureTag; assets = $assets
                    name = $releaseMetadata.name; body = $releaseMetadata.body; prerelease = $releaseMetadata.prerelease }
                if ($Scenario -eq 'wrong-approved-title') { $draft.name += ' changed' }
                if ($Scenario -eq 'missing-download-notes') { $draft.body = 'No verification instructions' }
                if ($Scenario -eq 'wrong-asset-digest') { $assets[0].digest = 'sha256:' + ('b' * 64) }
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
                if ($state.commits -ne 2 -or $state.drafts -ne 2 -or $state.downloads -ne 1 -or
                    $state.compares -ne 4 -or $state.runs -ne 4 -or $state.jobs -ne 2) {
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
            "api|repos/$fixtureRepository/releases/tags/$fixtureTag" {
                if ($state.patches -ne 1) { throw 'Read-back must follow publication.' }
                $state.publicReads++
                if ($Scenario -eq 'public-api-error') { $global:LASTEXITCODE = 1; return }
                $public = @{ id = $fixtureRelease; draft = $false; tag_name = $fixtureTag; assets = $assets
                    name = $releaseMetadata.name; body = $releaseMetadata.body; prerelease = $releaseMetadata.prerelease
                    published_at = '2026-01-01T00:00:00Z' }
                if ($Scenario -eq 'public-body-changed') { $public.body = 'Replaced public text' }
                if ($Scenario -eq 'public-assets-changed') { $assets[0].digest = 'sha256:' + ('b' * 64) }
                if ($Scenario -eq 'public-draft') { $public.draft = $true }
                return $public | ConvertTo-Json -Depth 7 -Compress
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
        switch -Wildcard ($Scenario) {
            'success' { & $publish 6>&1 | Out-Null }
            'unauthorized' { Must-Refuse { & $publish } 'requires a repository maintainer or administrator' }
            'no-key' { Must-Refuse { & $publish } 'Configure the RELEASE_APPROVAL_PUBLIC_KEY' }
            'unsigned' { Must-Refuse { & $publish } 'signed release approval is required' }
            'wrong-approved-title' { Must-Refuse { & $publish } 'metadata does not match the signed approval' }
            'wrong-asset-digest' { Must-Refuse { & $publish } 'attachment does not match the downloaded' }
            'missing-download-notes' { Must-Refuse { & $publish } 'download verification instructions' }
            'ci-*' { Must-Refuse { & $publish } $ciFailures[$Scenario] }
            'public-*' { Must-Refuse { & $publish } 'GitHub accepted publication*public verification failed' }
            default { Must-Refuse { & $publish } 'changed during verification' }
        }
    } finally {
        foreach ($key in $original.Keys) {
            [Environment]::SetEnvironmentVariable($key, $original[$key], 'Process')
        }
    }

    $expected = switch -Wildcard ($Scenario) {
        'success' { '1/3/2/1/1' }
        'unauthorized' { '1/0/0/0/0' }
        'no-key' { '0/0/0/0/0' }
        'unsigned' { '1/1/1/1/0' }
        'wrong-approved-title' { '1/1/1/1/0' }
        'wrong-asset-digest' { '1/1/1/1/0' }
        'missing-download-notes' { '1/1/1/1/0' }
        'tag-changed' { '1/2/2/1/0' }
        'ci-late-failure' { '1/1/1/1/0'; break }
        'ci-*' { '1/1/0/0/0' }
        'public-tag-changed' { '1/3/2/1/1'; break }
        'public-*' { '1/2/2/1/1' }
        default { '1/1/2/1/0' }
    }
    $calls = "$($state.permissions)/$($state.commits)/$($state.drafts)/$($state.downloads)/$($state.patches)"
    if ($calls -cne $expected) { throw "Unexpected $Scenario fixture calls: $calls, expected $expected." }
    $evidence = Join-Path $fixtureTemp 'release-publication.json'
    if ($Scenario -eq 'success') {
        $verified = Get-Content -LiteralPath $evidence -Raw | ConvertFrom-Json
        $bodyHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData(
            [Text.Encoding]::UTF8.GetBytes($releaseMetadata.body))).ToLowerInvariant()
        if ($state.publicReads -ne 1 -or $verified.commit -cne $FixtureCommit -or
            $verified.body_sha256 -cne $bodyHash -or $verified.assets.Count -ne $assets.Count) {
            throw 'Missing or incorrect public release verification evidence.'
        }
    } elseif (Test-Path -LiteralPath $evidence) {
        throw 'A failed verification produced evidence of a successful release.'
    }
}

try {
    # These small attachments contain fixture bytes. Replace only the
    # extraction adapter; the real approval and publication logic runs below.
    $acceptanceScripts = Join-Path $approvalScripts 'acceptance'
    New-Item -ItemType Directory -Path $acceptanceScripts -Force | Out-Null
    Copy-Item -LiteralPath @("$PSScriptRoot/release-approval.ps1", "$PSScriptRoot/publish-release-draft.ps1",
        "$PSScriptRoot/require-release-ci.ps1", "$PSScriptRoot/release-notes-policy.ps1") -Destination $approvalScripts
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
    Test-PortableSmoke (Join-Path $root 'portable-smoke')
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
        'title-changed', 'body-changed', 'prerelease-changed', 'wrong-asset-digest',
        'public-body-changed', 'public-assets-changed', 'public-draft', 'public-tag-changed', 'public-api-error') {
        Test-PublishDraft $scenario 'admin' $commit $bundle.FullName $approval $publicKey $root
    }
    foreach ($scenario in $ciFailures.Keys) {
        Test-PublishDraft $scenario 'maintain' $commit $bundle.FullName $approval $publicKey $root
    }

    $signedBody = '### 下载与校验' + ($releaseBody -split '### 下载与校验', 2)[1]
    Assert-ReleaseDownloadNotes -Body $signedBody -Version '1.4.1'
    Must-Refuse { Assert-ReleaseDownloadNotes -Body $signedBody -Version '1.4.1' -Unsigned } 'download verification instructions'
    $metadataFixture = $releaseMetadata.Clone()
    $metadataFixture.body = "<!--$releaseBody-->"
    $metadataFixture | ConvertTo-Json | Set-Content -LiteralPath $metadataPath
    Must-Refuse { & $check @params @prepare -LaunchRecord $evidence } 'download verification instructions'

    $noNotesEnvelope = $approval | ConvertFrom-Json -AsHashtable
    $noNotesPayload = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($noNotesEnvelope.payload)) | ConvertFrom-Json -AsHashtable
    $noNotesPayload.publication.body_sha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData(
        [Text.Encoding]::UTF8.GetBytes('No verification instructions'))).ToLowerInvariant()
    $noNotesBytes = [Text.Encoding]::UTF8.GetBytes(($noNotesPayload | ConvertTo-Json -Depth 7 -Compress))
    $noNotesEnvelope.payload = [Convert]::ToBase64String($noNotesBytes)
    $noNotesEnvelope.signature = [Convert]::ToBase64String($rsa.SignData($noNotesBytes,
        [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pss))
    Test-PublishDraft 'missing-download-notes' 'maintain' $commit $bundle.FullName ($noNotesEnvelope | ConvertTo-Json -Compress) $publicKey $root
    Write-Host 'Draft publication passed the exact-commit CI, download notes and public read-back checks.'

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
