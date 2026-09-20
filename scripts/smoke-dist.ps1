# Smoke-check Vite production output after `npm run build`.
# Usage: pwsh -NoProfile -File scripts/smoke-dist.ps1
$ErrorActionPreference = "Stop"
$here = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$root = Split-Path -Parent $here
$dist = Join-Path $root "dist"
$index = Join-Path $dist "index.html"

if (-not (Test-Path $index)) {
  Write-Error "smoke-dist FAILED: missing $index (run npm run build first)"
}
$html = Get-Content $index -Raw
if ($html -notmatch "assets/") {
  Write-Error "smoke-dist FAILED: dist/index.html does not reference assets/"
}
$assets = Join-Path $dist "assets"
if (-not (Test-Path $assets)) {
  Write-Error "smoke-dist FAILED: missing $assets"
}
$js = Get-ChildItem $assets -Filter "index-*.js" -File -ErrorAction SilentlyContinue
if (-not $js -or $js.Count -lt 1) {
  Write-Error "smoke-dist FAILED: no assets/index-*.js main chunk"
}
Write-Host "smoke-dist OK: $($js[0].Name) + index.html" -ForegroundColor Green
