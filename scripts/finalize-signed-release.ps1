$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$signPathEnabled = $env:SIGNPATH_ENABLED -ceq 'true'
$updaterEnabled = $env:UPDATER_SIGNING_ENABLED -ceq 'true'
$bundle = Join-Path $root 'src-tauri/target/release/bundle'
$nsis = @(Get-ChildItem -LiteralPath (Join-Path $bundle 'nsis') -Filter '*-setup.exe')
$msi = @(Get-ChildItem -LiteralPath (Join-Path $bundle 'msi') -Filter '*.msi')
if ($nsis.Count -ne 1 -or $msi.Count -ne 1) { throw 'Expected exactly one final NSIS and one MSI installer.' }

try {
    if ($signPathEnabled -and $updaterEnabled) {
        foreach ($installer in @($nsis[0], $msi[0])) {
            & (Join-Path $PSScriptRoot 'verify-authenticode.ps1') -Paths @($installer.FullName)
        }
    }
    if ($updaterEnabled) {
        if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -or
            [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) -or
            [string]::IsNullOrWhiteSpace($env:TAURI_UPDATER_PUBLIC_KEY)) {
            throw 'Updater signing mode requires private key/password/public key.'
        }
        foreach ($installer in @($nsis[0], $msi[0])) {
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
        Write-Host 'Updater manifest and signatures generated and verified.'
    } else {
        foreach ($installer in @($nsis[0], $msi[0])) {
            Remove-Item -LiteralPath "$($installer.FullName).sig" -Force -ErrorAction SilentlyContinue
        }
        Remove-Item -LiteralPath (Join-Path $bundle 'latest.json') -Force -ErrorAction SilentlyContinue
        Write-Host 'Unsigned/manual mode: installer packages only; updater assets omitted.'
    }
} finally {
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}
