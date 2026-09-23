# Minimal app-binary smoke (T-R7-02 E2E skeleton).
#
# Checks that a release `remova.exe` exists and is non-zero size after
# `npm run tauri build` (or a prior cargo build --release).
#
# This is intentionally NOT a GUI E2E: full UI flows (R1–R15) need a real
# desktop driver. Wire Playwright + Tauri WebDriver (tauri-driver / WebdriverIO)
# here when the test runner is available. Until then this script is the
# extensible gate: keep assertions binary-level and add step markers below.
#
# Usage: pwsh -NoProfile -File scripts/smoke-app.ps1
#        npm run smoke:app
$ErrorActionPreference = "Stop"
$here = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$root = Split-Path -Parent $here

# Candidate release roots (first hit wins).
$candidates = @(
  (Join-Path $root "src-tauri/target/release/remova.exe"),
  (Join-Path $root "src-tauri/target/release/Remova.exe")
)

$exe = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) {
  Write-Error "smoke-app FAILED: remova.exe not found under src-tauri/target/release (run 'npm run tauri build' first). Looked for: $($candidates -join ', ')"
}

$info = Get-Item -LiteralPath $exe
if ($info.Length -le 0) {
  Write-Error "smoke-app FAILED: $($info.FullName) is 0 bytes"
}

# Extensibility markers for future Playwright + Tauri driver steps:
#   [ ] launch remova.exe with test profile
#   [ ] assert window title / main nav is present
#   [ ] drive software list → analyze → confirm dialog (R-ranges)
#   [ ] quit and assert no crash dialog
# See docs/reviews/2026-09-22/…复审与优化路线图.md (T-R7-02).

Write-Host "smoke-app OK: $($info.FullName) ($($info.Length) bytes)" -ForegroundColor Green
