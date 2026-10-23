# Release readiness gate. Updater signing is optional: when keys are absent,
# the workflow publishes unsigned installers and SHA256SUMS only.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if ($env:SIGNPATH_ENABLED -and $env:SIGNPATH_ENABLED -cnotin @('true', 'false')) {
    throw 'SIGNPATH_ENABLED must be true, false, or unset.'
}
$signPathEnabled = $env:SIGNPATH_ENABLED -ceq 'true'
if ($signPathEnabled) {
    foreach ($name in @('SIGNPATH_API_TOKEN', 'SIGNPATH_ORGANIZATION_ID', 'SIGNPATH_PROJECT_SLUG',
        'SIGNPATH_SIGNING_POLICY_SLUG', 'SIGNPATH_CERTIFICATE_THUMBPRINT')) {
        if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
            throw "SignPath is enabled but missing release setting: $name. Refusing to publish without code signing."
        }
    }
    if ($env:SIGNPATH_CERTIFICATE_THUMBPRINT -notmatch '^[A-Fa-f0-9]{40}$') {
        throw 'SIGNPATH_CERTIFICATE_THUMBPRINT must be the approved certificate SHA-1 thumbprint (40 hex digits).'
    }
}

$tag = $env:GITHUB_REF_NAME
if ($tag -notmatch '^v\d+\.\d+\.\d+$') { throw 'Releases require a stable vMAJOR.MINOR.PATCH tag.' }
if ([string]::IsNullOrWhiteSpace($env:GITHUB_OUTPUT)) { throw 'Missing GITHUB_OUTPUT.' }

$updaterNames = @('TAURI_UPDATER_PUBLIC_KEY', 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD')
$updaterPresent = @($updaterNames | Where-Object {
    -not [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($_))
}).Count
if ($updaterPresent -ne 0 -and $updaterPresent -ne $updaterNames.Count) {
    throw 'Updater signing requires all three settings, or none for unsigned/manual release.'
}
$updaterEnabled = $updaterPresent -eq $updaterNames.Count
# Unsigned/manual releases are supported, but must be requested explicitly:
# silently omitting updater assets after a key misconfiguration would publish
# an install-only release while clients keep waiting for an update.
$allowUnsigned = $env:ALLOW_UNSIGNED_RELEASE -ceq 'true'
if (-not $updaterEnabled -and -not $allowUnsigned) {
    throw 'No updater signing keys are configured. Set ALLOW_UNSIGNED_RELEASE=true for an explicit unsigned/manual release.'
}
$dir = Join-Path $root '.release-signing'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
if ($updaterEnabled) {
    # Check the public key format rather than embedding a placeholder into a release.
    $publicText = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:TAURI_UPDATER_PUBLIC_KEY.Trim()))
    $publicLines = $publicText.Trim() -split '\r?\n'
    if ($publicLines.Count -ne 2 -or $publicLines[0] -notmatch '^untrusted comment:' -or
        [Convert]::FromBase64String($publicLines[1]).Length -ne 42) { throw 'Invalid Tauri updater public key.' }
    # A passwordless (unencrypted) private key would make the password requirement moot.
    $privateText = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:TAURI_SIGNING_PRIVATE_KEY.Trim()))
    if ($privateText -notmatch 'minisign-encrypted-secret') { throw 'Updater private key must be password-encrypted (minisign-encrypted-secret).' }
    @{ plugins = @{ updater = @{ pubkey = $env:TAURI_UPDATER_PUBLIC_KEY.Trim() } } } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $dir 'tauri.release.json') -Encoding utf8NoBOM
    Write-Host 'Release mode: updater signing enabled.'
} else {
    # An empty overlay preserves the base Tauri config without enabling updater assets.
    '{}' | Set-Content -LiteralPath (Join-Path $dir 'tauri.release.json') -Encoding utf8NoBOM
    Write-Host 'Release mode: unsigned/manual; no updater manifest or signature assets.'
}
"version=$($tag.Substring(1))" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
"signpath-enabled=$($signPathEnabled.ToString().ToLowerInvariant())" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
"updater-enabled=$($updaterEnabled.ToString().ToLowerInvariant())" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
