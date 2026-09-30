# Regression: actual CLI signatures, the embedded public key, and changed bytes.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$temp = Join-Path ([IO.Path]::GetTempPath()) ("remova-signature-test-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
$originalPublic = $env:TAURI_UPDATER_PUBLIC_KEY
$originalPrivate = $env:TAURI_SIGNING_PRIVATE_KEY
$originalPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
try {
    $key = Join-Path $temp 'test.key'
    $output = & npx tauri signer generate --ci --password 'temporary-test-only' --write-keys $key 2>&1
    if ($LASTEXITCODE -ne 0) { throw 'Could not generate temporary test key.' }
    $env:TAURI_UPDATER_PUBLIC_KEY = (Get-Content -LiteralPath "$key.pub" -Raw).Trim()
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -LiteralPath $key -Raw).Trim()
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = 'temporary-test-only'
    $file = Join-Path $temp 'installer.exe'
    [IO.File]::WriteAllText($file, 'final signed installer fixture')
    $output = & npx tauri signer sign $file 2>&1
    if ($LASTEXITCODE -ne 0) { throw 'Could not sign fixture.' }
    & cargo run --quiet --locked --manifest-path (Join-Path $root 'src-tauri/Cargo.toml') --bin verify_update -- $file
    if ($LASTEXITCODE -ne 0) { throw 'Correct signature was rejected.' }
    [IO.File]::AppendAllText($file, 'changed after signing')
    $output = & cargo run --quiet --locked --manifest-path (Join-Path $root 'src-tauri/Cargo.toml') --bin verify_update -- $file 2>&1
    if ($LASTEXITCODE -eq 0) { throw 'Changed installer bytes were accepted.' }
    Write-Host 'OK: final bytes accepted; installer modified after signing rejected.'
} finally {
    $env:TAURI_UPDATER_PUBLIC_KEY = $originalPublic
    $env:TAURI_SIGNING_PRIVATE_KEY = $originalPrivate
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $originalPassword
    # The generated absolute directory must remain under the named temp parent.
    $resolved = [IO.Path]::GetFullPath($temp)
    $parent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if ($resolved.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase) -and
        [IO.Path]::GetFileName($resolved) -match '^remova-signature-test-[a-f0-9]{32}$') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
