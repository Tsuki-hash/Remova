# Smoke-check portable zip after `npm run package:portable` / tauri build.
# Usage: pwsh -NoProfile -File scripts/smoke-portable.ps1
$ErrorActionPreference = "Stop"
$here = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$root = Split-Path -Parent $here

$pkgPath = Join-Path $root "package.json"
if (-not (Test-Path $pkgPath)) {
  Write-Error "smoke-portable FAILED: missing $pkgPath"
}
$ver = (Get-Content $pkgPath -Raw | ConvertFrom-Json).version
$outDir = Join-Path $root "src-tauri/target/release/bundle/portable"
$zip = Join-Path $outDir "Remova_${ver}_x64-portable.zip"

if (-not (Test-Path $zip)) {
  Write-Error "smoke-portable FAILED: missing $zip (run npm run tauri build && npm run package:portable first)"
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
try {
  $names = @($archive.Entries | ForEach-Object { $_.FullName.Replace('\', '/') })
  foreach ($required in @("Remova.exe", "PORTABLE.txt")) {
    if (-not ($names | Where-Object { $_ -eq $required -or $_ -like "*/$required" })) {
      Write-Error "smoke-portable FAILED: zip missing $required. Entries: $($names -join ', ')"
    }
  }
  $portableEntry = $archive.Entries | Where-Object { $_.Name -eq "PORTABLE.txt" } | Select-Object -First 1
  $reader = New-Object System.IO.StreamReader($portableEntry.Open())
  try { $portableText = $reader.ReadToEnd() } finally { $reader.Dispose() }
} finally {
  $archive.Dispose()
}

if ($portableText -notmatch [regex]::Escape($ver)) {
  Write-Error "smoke-portable FAILED: PORTABLE.txt does not mention version $ver"
}
if ($portableText -notmatch "WebView2") {
  Write-Error "smoke-portable FAILED: PORTABLE.txt missing WebView2 runtime note"
}
Write-Host "smoke-portable OK: $zip (Remova.exe + PORTABLE.txt + WebView2 + $ver)" -ForegroundColor Green
