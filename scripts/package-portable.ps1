# Package a portable Remova zip from the built release binary.
# Usage: pwsh -File scripts/package-portable.ps1
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
# Optional runtime note for portable users
@"
Remova $ver portable
- Double-click Remova.exe (no install).
- Backups: %PROGRAMDATA%\Remova\Backup
- Prefer the NSIS/MSI installer for Start menu & uninstaller.
"@ | Set-Content -Path (Join-Path $stage "PORTABLE.txt") -Encoding utf8
$zip = Join-Path $outDir "Remova_${ver}_x64-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Write-Host "Wrote $zip"
