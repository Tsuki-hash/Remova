# Readiness gate. No private key is used before the final updater-signing step.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
foreach ($name in @('SIGNPATH_API_TOKEN', 'SIGNPATH_ORGANIZATION_ID', 'SIGNPATH_PROJECT_SLUG',
    'SIGNPATH_SIGNING_POLICY_SLUG', 'SIGNPATH_CERTIFICATE_THUMBPRINT', 'TAURI_UPDATER_PUBLIC_KEY', 'TAURI_SIGNING_PRIVATE_KEY')) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
        throw "Missing release setting: $name. Configure SignPath before tagging a release."
    }
}
if ($env:SIGNPATH_CERTIFICATE_THUMBPRINT -notmatch '^[A-Fa-f0-9]{40}$') {
    throw 'SIGNPATH_CERTIFICATE_THUMBPRINT must be the approved certificate SHA-1 thumbprint (40 hex digits).'
}
# Check the public key format rather than embedding a placeholder into a release.
$publicText = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:TAURI_UPDATER_PUBLIC_KEY.Trim()))
$publicLines = $publicText.Trim() -split '\r?\n'
if ($publicLines.Count -ne 2 -or $publicLines[0] -notmatch '^untrusted comment:' -or
    [Convert]::FromBase64String($publicLines[1]).Length -ne 42) { throw 'Invalid Tauri updater public key.' }
$dir = Join-Path $root '.release-signing'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
@{ plugins = @{ updater = @{ pubkey = $env:TAURI_UPDATER_PUBLIC_KEY.Trim() } } } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $dir 'tauri.release.json') -Encoding utf8NoBOM
$tag = $env:GITHUB_REF_NAME
if ($tag -notmatch '^v\d+\.\d+\.\d+$') { throw 'Signed releases require a stable vMAJOR.MINOR.PATCH tag.' }
"version=$($tag.Substring(1))" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
