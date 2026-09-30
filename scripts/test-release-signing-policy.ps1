# Exercise release policy in an isolated fixture tree. No build, real keys, or network.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$temp = Join-Path ([IO.Path]::GetTempPath()) ('remova-release-policy-test-' + [Guid]::NewGuid().ToString('N'))
$names = @('SIGNPATH_ENABLED', 'SIGNPATH_API_TOKEN', 'SIGNPATH_ORGANIZATION_ID', 'SIGNPATH_PROJECT_SLUG',
    'SIGNPATH_SIGNING_POLICY_SLUG', 'SIGNPATH_CERTIFICATE_THUMBPRINT', 'TAURI_UPDATER_PUBLIC_KEY',
    'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'GITHUB_OUTPUT', 'GITHUB_REF_NAME', 'GITHUB_REPOSITORY')
$saved = @{}
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
$passed = 0
$policyState = @{ SignCalls = 0; VerifyCalls = 0; FailVerification = $false }
function Assert-Throws([scriptblock]$Call, [string]$Expected) {
    try { & $Call } catch {
        if ($_.Exception.Message -notlike "*$Expected*") { throw }
        return
    }
    throw "Expected failure containing: $Expected"
}
# Stubs verify orchestration only. Real cryptographic verification has its own regression.
function npx {
    $policyState.SignCalls++
    Set-Content -LiteralPath "$($args[-1]).sig" -Value 'fixture-signature' -NoNewline
    $global:LASTEXITCODE = 0
}
function cargo {
    $policyState.VerifyCalls++
    $global:LASTEXITCODE = if ($policyState.FailVerification) { 1 } else { 0 }
}
try {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $null) }
    New-Item -ItemType Directory -Path (Join-Path $temp 'scripts') -Force | Out-Null
    foreach ($file in @('prepare-signed-release.ps1', 'finalize-signed-release.ps1', 'verify-authenticode.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination (Join-Path $temp 'scripts')
    }
    Set-Content -LiteralPath (Join-Path $temp 'package.json') -Value '{"version":"1.3.0"}'
    $packet = [byte[]]::new(42)
    $packet[0] = 69; $packet[1] = 100
    $keyText = "untrusted comment: release policy test`n" + [Convert]::ToBase64String($packet)
    $env:TAURI_UPDATER_PUBLIC_KEY = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($keyText))
    # The policy rejects passwordless (unencrypted) private keys: the fixture
    # must be a base64 payload carrying the minisign-encrypted-secret marker.
    $script:fixturePrivateKey = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(
        "untrusted comment: release policy fixture`nminisign-encrypted-secret: Zml4dHVyZQ=="))
    $env:TAURI_SIGNING_PRIVATE_KEY = $script:fixturePrivateKey
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = 'test-placeholder'
    $env:GITHUB_REF_NAME = 'v1.3.0'
    $env:GITHUB_REPOSITORY = 'Tsuki-hash/Remova'
    $env:GITHUB_OUTPUT = Join-Path $temp 'outputs.txt'
    $prepare = Join-Path $temp 'scripts/prepare-signed-release.ps1'
    & $prepare
    if ((Get-Content -LiteralPath $env:GITHUB_OUTPUT -Raw) -notmatch 'signpath-enabled=false') { throw 'SignPath must default to disabled.' }
    $passed++

    $env:SIGNPATH_ENABLED = 'false'
    $env:SIGNPATH_API_TOKEN = 'leftover-token'
    $env:SIGNPATH_CERTIFICATE_THUMBPRINT = 'invalid-leftover-thumbprint'
    & $prepare
    $passed++

    $env:TAURI_SIGNING_PRIVATE_KEY = ''
    Assert-Throws { & $prepare } 'TAURI_SIGNING_PRIVATE_KEY'
    $env:TAURI_SIGNING_PRIVATE_KEY = $script:fixturePrivateKey
    $passed++

    # A private key WITHOUT the encrypted-secret marker must be refused even
    # when it is valid base64 (passwordless keys make the password moot).
    $env:TAURI_SIGNING_PRIVATE_KEY = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(
        "untrusted comment: passwordless fixture`nplain-unencrypted-payload"))
    Assert-Throws { & $prepare } 'password-encrypted'
    $env:TAURI_SIGNING_PRIVATE_KEY = $script:fixturePrivateKey
    $passed++

    $env:SIGNPATH_ENABLED = 'true'
    Assert-Throws { & $prepare } 'SIGNPATH_ORGANIZATION_ID'
    $passed++
    foreach ($name in @('SIGNPATH_ORGANIZATION_ID', 'SIGNPATH_PROJECT_SLUG', 'SIGNPATH_SIGNING_POLICY_SLUG')) {
        [Environment]::SetEnvironmentVariable($name, 'test-placeholder')
    }
    Assert-Throws { & $prepare } 'thumbprint'
    $passed++
    $env:SIGNPATH_CERTIFICATE_THUMBPRINT = 'A' * 40
    & $prepare
    if ((Get-Content -LiteralPath $env:GITHUB_OUTPUT -Raw) -notmatch 'signpath-enabled=true') { throw 'Enabled SignPath was not emitted.' }
    $passed++

    $env:SIGNPATH_ENABLED = 'tru'
    Assert-Throws { & $prepare } 'SIGNPATH_ENABLED'
    $passed++
    $env:SIGNPATH_ENABLED = 'false'
    $public = $env:TAURI_UPDATER_PUBLIC_KEY
    $env:TAURI_UPDATER_PUBLIC_KEY = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('invalid key'))
    Assert-Throws { & $prepare } 'public key'
    $env:TAURI_UPDATER_PUBLIC_KEY = $public
    $passed++

    # finalize drops the private-key material from the environment after
    # signing; restore the fixture keys for the remaining finalize cases.
    $env:TAURI_SIGNING_PRIVATE_KEY = $script:fixturePrivateKey
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = 'test-placeholder'
    $bundle = Join-Path $temp 'src-tauri/target/release/bundle'
    foreach ($kind in @('nsis', 'msi')) { New-Item -ItemType Directory -Path (Join-Path $bundle $kind) -Force | Out-Null }
    Set-Content -LiteralPath (Join-Path $bundle 'nsis/Remova_1.3.0_x64-setup.exe') -Value 'unsigned fixture'
    Set-Content -LiteralPath (Join-Path $bundle 'msi/Remova_1.3.0_x64.msi') -Value 'unsigned fixture'
    $finalize = Join-Path $temp 'scripts/finalize-signed-release.ps1'
    $policyState.SignCalls = 0; $policyState.VerifyCalls = 0; $policyState.FailVerification = $false
    & $finalize
    $manifest = Join-Path $bundle 'latest.json'
    $json = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
    if ($policyState.SignCalls -ne 2 -or $policyState.VerifyCalls -ne 2 -or
        $json.platforms.'windows-x86_64-nsis'.signature -ne 'fixture-signature') { throw 'Updater-only mode must sign and verify both installers.' }
    $passed++

    Remove-Item -LiteralPath $manifest
    $env:SIGNPATH_ENABLED = 'true'
    Assert-Throws { & $finalize } 'Authenticode'
    if (Test-Path -LiteralPath $manifest) { throw 'Unsigned Windows binaries were published with SignPath enabled.' }
    $passed++

    $env:SIGNPATH_ENABLED = 'false'
    $policyState.FailVerification = $true
    Assert-Throws { & $finalize } 'Updater signature does not match'
    if (Test-Path -LiteralPath $manifest) { throw 'Invalid updater signature produced a release manifest.' }
    $passed++
    Write-Host "OK: $passed release policy cases passed (no build or packaging)."
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    $resolved = [IO.Path]::GetFullPath($temp)
    $parent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if ($resolved.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase) -and
        [IO.Path]::GetFileName($resolved) -match '^remova-release-policy-test-[a-f0-9]{32}$' -and
        (Test-Path -LiteralPath $resolved)) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
