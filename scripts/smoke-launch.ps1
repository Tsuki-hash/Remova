# REV-QA-07 (1.2.2): real launch smoke — start the built exe, verify it stays
# alive for a few seconds (WebView2 init + shell), then kill it.
# Requires: a release build (npm run tauri build) and WebView2 Runtime.
# Note: the app registers a single-instance plugin — close any running Remova first.
# Usage: pwsh -NoProfile -File scripts/smoke-launch.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$exeCandidates = @(
  (Join-Path $root "src-tauri\target\release\remova.exe"),
  (Join-Path $root "src-tauri\target\release\Remova.exe")
)
$exe = $exeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) {
  Write-Error "Build first: npm run tauri build. Looked for: $($exeCandidates -join ', ')"
}

$proc = Start-Process -FilePath $exe -PassThru
try {
  Start-Sleep -Seconds 8
  if ($proc.HasExited) {
    Write-Error "smoke-launch FAILED: process exited within 8s (code $($proc.ExitCode))"
  }
  Write-Host "smoke-launch OK: process alive after 8s (pid $($proc.Id))"
} finally {
  if (-not $proc.HasExited) {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
  }
}
