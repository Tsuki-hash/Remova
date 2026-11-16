# Smoke-check portable zip after `npm run package:portable` / tauri build.
# Usage: pwsh -NoProfile -File scripts/smoke-portable.ps1
$ErrorActionPreference = "Stop"
. "$PSScriptRoot/acceptance/acceptance-common.ps1"
$root = Split-Path -Parent $PSScriptRoot

$pkgPath = Join-Path $root "package.json"
if (-not (Test-Path $pkgPath)) {
  Write-Error "smoke-portable FAILED: missing $pkgPath"
}
$ver = (Get-Content $pkgPath -Raw | ConvertFrom-Json).version
$releaseDir = Join-Path $root "src-tauri/target/release"
$outDir = Join-Path $releaseDir "bundle/portable"
$zip = Join-Path $outDir "Remova_${ver}_x64-portable.zip"

if (-not (Test-Path $zip)) {
  Write-Error "smoke-portable FAILED: missing $zip (run npm run tauri build && npm run package:portable first)"
}

$buildExe = @('remova.exe', 'Remova.exe') | ForEach-Object { Join-Path $releaseDir $_ } |
  Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $buildExe) { throw 'smoke-portable FAILED: the built Remova.exe is missing.' }
$buildHash = (Get-FileHash -LiteralPath $buildExe -Algorithm SHA256).Hash.ToLowerInvariant()
if ($env:REMOVA_SMOKED_EXECUTABLE_SHA256 -and $buildHash -cne $env:REMOVA_SMOKED_EXECUTABLE_SHA256) {
  throw 'smoke-portable FAILED: the built Remova.exe differs from the executable used for launch smoke.'
}
$distributionHashes = @{}
foreach ($name in @('LICENSE', 'THIRD-PARTY-NOTICES.md')) {
  $distributionHashes[$name] = (Get-FileHash -LiteralPath (Join-Path $root $name) -Algorithm SHA256).Hash.ToLowerInvariant()
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
try {
  $portableExeHash = Get-PortableArchiveExeHash $archive $distributionHashes
  if ($portableExeHash -cne $buildHash) {
    throw 'smoke-portable FAILED: archived Remova.exe differs from the built executable.'
  }
  $portableEntries = @($archive.Entries | Where-Object { $_.FullName -ieq 'PORTABLE.txt' })
  if ($portableEntries.Count -ne 1) { throw 'smoke-portable FAILED: expected one PORTABLE.txt at the root.' }
  $reader = [IO.StreamReader]::new($portableEntries[0].Open())
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
Write-Host "smoke-portable OK: $zip (Remova.exe SHA-256 $portableExeHash)" -ForegroundColor Green
