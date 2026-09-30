# Readiness gate. No private key is used before the final updater-signing step.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
foreach ($name in @('TAURI_UPDATER_PUBLIC_KEY', 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD')) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
        throw "Missing release setting: $name. Configure updater signing before tagging a release."
    }
}
# Explicit opt-in: leftover SignPath settings must not accidentally enable signing.
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
# Check the public key format rather than embedding a placeholder into a release.
$publicText = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:TAURI_UPDATER_PUBLIC_KEY.Trim()))
$publicLines = $publicText.Trim() -split '\r?\n'
if ($publicLines.Count -ne 2 -or $publicLines[0] -notmatch '^untrusted comment:' -or
    [Convert]::FromBase64String($publicLines[1]).Length -ne 42) { throw 'Invalid Tauri updater public key.' }
# A passwordless (unencrypted) private key would make the password requirement
# moot - refuse it before anything is signed with it.
$privateText = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:TAURI_SIGNING_PRIVATE_KEY.Trim()))
if ($privateText -notmatch 'minisign-encrypted-secret') { throw 'Updater private key must be password-encrypted (minisign-encrypted-secret).' }
$tag = $env:GITHUB_REF_NAME
if ($tag -notmatch '^v\d+\.\d+\.\d+$') { throw 'Updater releases require a stable vMAJOR.MINOR.PATCH tag.' }
if ([string]::IsNullOrWhiteSpace($env:GITHUB_OUTPUT)) { throw 'Missing GITHUB_OUTPUT.' }
$dir = Join-Path $root '.release-signing'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
@{ plugins = @{ updater = @{ pubkey = $env:TAURI_UPDATER_PUBLIC_KEY.Trim() } } } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $dir 'tauri.release.json') -Encoding utf8NoBOM
"version=$($tag.Substring(1))" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
"signpath-enabled=$($signPathEnabled.ToString().ToLowerInvariant())" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
if ($signPathEnabled) {
    Write-Host 'Release mode: updater signing and SignPath Authenticode signing.'
} else {
    Write-Host 'Release mode: updater signing only; Windows binaries will not have a trusted publisher signature.'
}
