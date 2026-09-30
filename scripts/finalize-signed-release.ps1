$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ($env:SIGNPATH_ENABLED -and $env:SIGNPATH_ENABLED -cnotin @('true', 'false')) {
    throw 'SIGNPATH_ENABLED must be true, false, or unset.'
}
$signPathEnabled = $env:SIGNPATH_ENABLED -ceq 'true'
$bundle = Join-Path $root 'src-tauri/target/release/bundle'
$nsis = @(Get-ChildItem -LiteralPath (Join-Path $bundle 'nsis') -Filter '*-setup.exe')
$msi = @(Get-ChildItem -LiteralPath (Join-Path $bundle 'msi') -Filter '*.msi')
if ($nsis.Count -ne 1 -or $msi.Count -ne 1) { throw 'Expected exactly one final NSIS and one MSI installer.' }
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -or
    [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) -or
    [string]::IsNullOrWhiteSpace($env:TAURI_UPDATER_PUBLIC_KEY)) { throw 'Missing updater private key/password/public key.' }
# Sign final bytes: SignPath output when enabled, otherwise the original installers.
foreach ($installer in @($nsis[0], $msi[0])) {
    if ($signPathEnabled) {
        & (Join-Path $PSScriptRoot 'verify-authenticode.ps1') -Paths @($installer.FullName)
    }
    & npx tauri signer sign $installer.FullName
    if ($LASTEXITCODE -ne 0) { throw 'Tauri updater signing failed.' }
    & cargo run --quiet --locked --manifest-path (Join-Path $root 'src-tauri/Cargo.toml') --bin verify_update -- $installer.FullName
    if ($LASTEXITCODE -ne 0) { throw 'Updater signature does not match the public key embedded in this release.' }
}
$version = (Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
if ($env:GITHUB_REF_NAME -cne "v$version") { throw 'Version/tag mismatch.' }
$url = "https://github.com/$($env:GITHUB_REPOSITORY)/releases/download/$($env:GITHUB_REF_NAME)/$($nsis[0].Name)"
@{
    version = $version
    notes = 'See the release page for changes.'
    pub_date = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = @{ 'windows-x86_64-nsis' = @{
        url = $url
        signature = (Get-Content -LiteralPath "$($nsis[0].FullName).sig" -Raw).Trim()
    } }
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $bundle 'latest.json') -Encoding utf8NoBOM
