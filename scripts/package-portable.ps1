# Package a portable Remova zip from the built release binary.
# Usage: pwsh -NoProfile -File scripts/package-portable.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "src-tauri\target\release\remova.exe"
if (-not (Test-Path $exe)) {
  Write-Error "Build first: npm run tauri build (missing $exe)"
}
$ver = (Get-Content (Join-Path $root "package.json") -Raw | ConvertFrom-Json).version
$outDir = Join-Path $root "src-tauri\target\release\bundle\portable"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$stage = Join-Path $outDir "Remova-$ver"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $exe (Join-Path $stage "Remova.exe")
# D-N1: portable runtime notes (WebView2 + backups + no installer)
@"
Remova $ver portable (x64)
- Double-click Remova.exe (no install). Prefer NSIS/MSI for Start menu entry.
- Runtime: Microsoft Edge WebView2 Runtime must already be installed (Windows 10/11 usually include it).
  If the app fails to start, install WebView2 from Microsoft and retry.
- Backups: %PROGRAMDATA%\Remova\Backup
- Version in this package: $ver
"@ | Set-Content -Path (Join-Path $stage "PORTABLE.txt") -Encoding utf8
$zip = Join-Path $outDir "Remova_${ver}_x64-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path "$stage\*" -DestinationPath $zip
# Clean stage after zip to avoid leftover unversioned trees (D-N1).
Remove-Item $stage -Recurse -Force
Write-Host "Wrote $zip"
