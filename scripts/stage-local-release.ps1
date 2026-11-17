# Collect and verify the five unsigned attachments without contacting GitHub.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^v\d+\.\d+\.\d+$')][string]$Tag,
    [Parameter(Mandatory)][string]$BuildEvidence,
    [Parameter(Mandatory)][string]$OutDir,
    [Parameter(Mandatory)][string]$SevenZip,
    [string]$WixDark = (Join-Path $env:LOCALAPPDATA 'tauri/WixTools314/dark.exe')
)

$ErrorActionPreference = 'Stop'
$version = $Tag.Substring(1)
$destination = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutDir)
if (Test-Path -LiteralPath $destination) { throw 'Choose a new release staging directory.' }
$build = Get-Content -LiteralPath $BuildEvidence -Raw | ConvertFrom-Json -AsHashtable
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$pins = [Collections.Generic.List[IO.FileStream]]::new()
Push-Location $root
try {
    $commit = git rev-parse HEAD
    if ($LASTEXITCODE -or $build.schema_version -ne 2 -or $build.commit -cne $commit) {
        throw 'The build record must refer to this checkout commit.'
    }
    $dirty = git status --porcelain --untracked-files=no
    if ($LASTEXITCODE -or $dirty) { throw 'Stage the candidate from the clean build checkout.' }
    $config = Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json
    if ($config.version -cne $version -or $config.plugins.updater.pubkey) {
        throw 'This procedure requires the matching unsigned/manual-download configuration.'
    }
    $expectedInputs = @('package-lock.json', 'src-tauri/Cargo.lock', 'src-tauri/tauri.conf.json',
        'src-tauri/windows/installer.nsi', 'src-tauri/windows/installer-hooks.nsh')
    $expectedOutputs = @('remova.exe', "bundle/nsis/Remova_${version}_x64-setup.exe", "bundle/msi/Remova_${version}_x64_zh-CN.msi")
    foreach ($entry in @(@{ Expected = $expectedInputs; Actual = $build.inputs_sha256 },
        @{ Expected = $expectedOutputs; Actual = $build.outputs_sha256 })) {
        if ($entry.Actual -isnot [Collections.IDictionary] -or
            @(Compare-Object @($entry.Actual.Keys) $entry.Expected -CaseSensitive).Count) {
            throw 'The build record is missing the exact candidate input or output inventory.'
        }
        foreach ($name in $entry.Expected) {
            if ($entry.Actual[$name] -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid build digest.' }
        }
    }
    foreach ($file in $expectedInputs) {
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ine $build.inputs_sha256[$file]) {
            throw "The build input changed: $file."
        }
    }
    foreach ($file in $expectedOutputs) {
        $pin = [IO.File]::Open((Join-Path $root "src-tauri/target/release/$file"), 'Open', 'Read', 'Read')
        $pins.Add($pin)
        if ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($pin)).ToLowerInvariant() -cne $build.outputs_sha256[$file]) {
            throw "The candidate output does not match the build record: $file."
        }
    }
    & "$PSScriptRoot/package-portable.ps1"
    & "$PSScriptRoot/smoke-portable.ps1"
    if ($LASTEXITCODE) { throw 'Portable package smoke failed.' }
    & "$PSScriptRoot/make-checksums.ps1"
    node "$PSScriptRoot/make-sbom.mjs"
    if ($LASTEXITCODE) { throw 'SBOM generation failed.' }
    & "$PSScriptRoot/extract-release-notes.ps1" -Tag $Tag -CheckDownloadNotes -Unsigned
    if ($LASTEXITCODE) { throw 'Release notes validation failed.' }

    $attachments = (New-Item -ItemType Directory -Path (Join-Path $destination 'attachments') -Force).FullName
    $bundle = 'src-tauri/target/release/bundle'
    $names = @("nsis/Remova_${version}_x64-setup.exe", "msi/Remova_${version}_x64_zh-CN.msi", "portable/Remova_${version}_x64-portable.zip", 'SHA256SUMS.txt', 'SBOM.cdx.json')
    foreach ($name in $names) {
        Copy-Item -LiteralPath (Join-Path $bundle $name) -Destination $attachments
    }
    $reference = & "$PSScriptRoot/acceptance/get-package-exe-reference.ps1" -Artifacts $attachments -CandidateVersion $version -SevenZip $SevenZip -WixDark $WixDark
    $actual = $reference | ConvertFrom-Json -AsHashtable
    if ($actual.portable.executable_sha256 -cne $build.outputs_sha256['remova.exe'] -or
        $actual.nsis.package_sha256 -cne $build.outputs_sha256["bundle/nsis/Remova_${version}_x64-setup.exe"] -or
        $actual.msi.package_sha256 -cne $build.outputs_sha256["bundle/msi/Remova_${version}_x64_zh-CN.msi"]) {
        throw 'The staged attachments differ from the locked build outputs.'
    }
    $reference | Set-Content -LiteralPath (Join-Path $destination 'package-exe-reference.json') -Encoding utf8
    $body = Get-Content -Raw -LiteralPath ".release-notes/$Tag.md"
    Copy-Item -LiteralPath ".release-notes/$Tag.md" -Destination (Join-Path $destination 'release-notes.md')
    [ordered]@{ name = "Remova $version"; body = $body; prerelease = $false; make_latest = 'true' } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'release-metadata.json') -Encoding utf8
    $build | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $destination 'build-inputs.json') -Encoding utf8
    Write-Host "Staged five release attachments for $Tag ($commit) in $destination."
} finally {
    foreach ($pin in $pins) { $pin.Dispose() }
    Pop-Location
}
