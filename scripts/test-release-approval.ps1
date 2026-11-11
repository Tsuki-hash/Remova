$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance/acceptance-common.ps1"
$temp = (Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path
$root = Join-Path $temp "remova-release-approval-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $root | Out-Null

function Must-Refuse([scriptblock]$Action, [string]$Reason) {
    try { & $Action | Out-Null } catch {
        if ($_.Exception.Message -like "*$Reason*") { return }
        throw
    }
    throw "Expected refusal: $Reason"
}

function Test-PublishDraft(
    [string]$Scenario,
    [string]$FixtureRole,
    [string]$FixtureCommit,
    [string]$FixtureBundle,
    [string]$FixtureApproval,
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
                $draft = @{ id = $fixtureRelease; draft = $true; tag_name = $fixtureTag; assets = $assets }
                if ($state.drafts -eq 2) {
                    if ($Scenario -eq 'asset-changed') { $assets[0].id += 100 }
                    if ($Scenario -eq 'draft-replaced') { $draft.id++ }
                }
                $earlier = @{ id = 42; draft = $false; tag_name = 'v1.4.0'; assets = @() }
                return ConvertTo-Json -InputObject @(@($earlier), @($draft)) -Depth 7 -Compress
            }
            "api|--method|PATCH|repos/$fixtureRepository/releases/$fixtureRelease|-F|draft=false|--silent" {
                if ($state.commits -ne 2 -or $state.drafts -ne 2 -or $state.downloads -ne 1) {
                    throw 'The release was patched before verification completed.'
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
    }
    $original = @{}
    try {
        foreach ($key in $overrides.Keys) {
            $original[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
            [Environment]::SetEnvironmentVariable($key, $overrides[$key], 'Process')
        }
        $publish = "$PSScriptRoot/publish-release-draft.ps1"
        switch ($Scenario) {
            'success' { & $publish 6>&1 | Out-Null }
            'unauthorized' { Must-Refuse { & $publish } 'requires a repository maintainer or administrator' }
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
        'tag-changed' { '1/2/2/1/0' }
        default { '1/1/2/1/0' }
    }
    $calls = "$($state.permissions)/$($state.commits)/$($state.drafts)/$($state.downloads)/$($state.patches)"
    if ($calls -cne $expected) { throw "Unexpected $Scenario fixture calls: $calls, expected $expected." }
}

try {
    $tag = 'v1.4.1'
    $commit = 'a' * 40
    $bundle = New-Item -ItemType Directory -Path (Join-Path $root 'bundle')
    $names = [ordered]@{
        nsis = 'Remova_1.4.1_x64-setup.exe'
        msi = 'Remova_1.4.1_x64_zh-CN.msi'
        portable = 'Remova_1.4.1_x64-portable.zip'
    }
    $artifacts = [ordered]@{}
    $sums = foreach ($channel in $names.Keys) {
        $name = $names[$channel]
        $file = Join-Path $bundle.FullName $name
        Set-Content -LiteralPath $file -Value "fixture $channel"
        $hash = (Get-FileHash -LiteralPath $file).Hash.ToLowerInvariant()
        $artifacts[$channel] = @{ name = $name; sha256 = $hash }
        "$hash  $name"
    }
    Set-Content -LiteralPath (Join-Path $bundle.FullName 'SHA256SUMS.txt') -Value $sums
    Set-Content -LiteralPath (Join-Path $bundle.FullName 'SBOM.cdx.json') -Value '{"bomFormat":"CycloneDX","metadata":{"component":{"version":"1.4.1"}},"components":[{"name":"fixture"}]}'

    $evidence = foreach ($family in 'windows-10', 'windows-11') {
        foreach ($suite in 'client', 'elevated') {
            $dir = New-Item -ItemType Directory -Path (Join-Path $root "$family-$suite")
            $log = Join-Path $dir.FullName 'observations.txt'
            Set-Content -LiteralPath $log -Value 'fixture observation'
            $summary = [ordered]@{
                schema_version = 1
                kind = 'remova-acceptance'
                candidate_commit = $commit
                candidate_version = '1.4.1'
                os_family = $family
                os_version = if ($family -eq 'windows-10') { '10.0.19045' } else { '10.0.26200' }
                architecture = 'x64'
                suite = $suite
                elevated = $suite -eq 'elevated'
                webview2_version = '1.2.3.4'
                executor = 'fixture'
                artifacts = $artifacts
                passed = $true
                checks = @(Get-AcceptanceChecks $suite | ForEach-Object {
                    @{ id = $_.id; actual = 'observed'; status = 'pass'; evidence = @{
                        name = 'observations.txt'; sha256 = (Get-FileHash -LiteralPath $log).Hash.ToLowerInvariant()
                    } }
                })
            }
            $file = Join-Path $dir.FullName 'acceptance-summary.json'
            $summary | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $file
            $file
        }
    }
    $check = "$PSScriptRoot/release-approval.ps1"
    $params = @{ Tag = $tag; Commit = $commit; BundleDir = $bundle.FullName }
    $approval = & $check @params -Acceptance $evidence -Reviewed
    & $check @params -ApprovalJson $approval
    Must-Refuse { & $check @params -Acceptance $evidence[0..2] -Reviewed } 'Acceptance matrix'
    Must-Refuse { & $check @params -ApprovalJson '{}' } 'different release or commit'

    Test-PublishDraft 'success' 'maintain' $commit $bundle.FullName $approval $root
    Test-PublishDraft 'unauthorized' 'write' $commit $bundle.FullName $approval $root
    foreach ($scenario in 'asset-changed', 'draft-replaced', 'tag-changed') {
        Test-PublishDraft $scenario 'admin' $commit $bundle.FullName $approval $root
    }
    Write-Host 'Draft publication fixture passed the maintainer, permission, changed-asset, replaced-draft and changed-tag checks.'

    Add-Content -LiteralPath (Join-Path $bundle.FullName 'SBOM.cdx.json') -Value ' '
    Must-Refuse { & $check @params -ApprovalJson $approval } 'draft attachment changed'
    $log = Join-Path (Split-Path -Parent $evidence[0]) 'observations.txt'
    Add-Content -LiteralPath $log -Value 'changed after review'
    Must-Refuse { & $check @params -Acceptance $evidence -Reviewed } 'Evidence changed'
    Write-Host 'Release approval passed the complete matrix, missing-run, wrong-approval, changed-asset and changed-evidence checks.'
} finally {
    $resolved = (Resolve-Path -LiteralPath $root).Path
    if ((Split-Path -Parent $resolved) -eq $temp -and (Split-Path -Leaf $resolved) -match '^remova-release-approval-[a-f0-9]{32}$') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
