# Build the GitHub Release body from the CHANGELOG section that matches the release tag.
# docs/ is not committed, so the workflow cannot read a curated per-release notes file from the
# checkout; CHANGELOG.md is the single published source. An absent or empty section fails the job,
# because otherwise the Release would silently fall back to GitHub-generated notes (a raw commit list).
# Usage: pwsh -NoProfile -File scripts/extract-release-notes.ps1 [-Tag v1.1.1]
param([string]$Tag = "")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not $Tag) { $Tag = $env:GITHUB_REF_NAME }
if ($Tag -notmatch '^v?([0-9]+\.[0-9]+\.[0-9]+)$') {
  Write-Error "cannot resolve a release tag from '$Tag' (expected vX.Y.Z or GITHUB_REF_NAME)"
}
$ver = $Matches[1]

$lines = @(Get-Content "CHANGELOG.md")
$headPattern = '^## \[' + [regex]::Escape($ver) + '\]'
$start = -1
for ($i = 0; $i -lt $lines.Count; $i++) {
  if ($lines[$i] -match $headPattern) { $start = $i; break }
}
if ($start -lt 0) {
  Write-Error "CHANGELOG.md has no '## [$ver]' section - add it before releasing $Tag."
}

$end = $lines.Count
for ($i = $start + 1; $i -lt $lines.Count; $i++) {
  if ($lines[$i] -match '^## ') { $end = $i; break }
}

$body = @()
if ($lines[$start] -match '^## \[[0-9.]+\]\s*-?\s*(.*)$') {
  $date = $Matches[1].Trim()
  $body += if ($date) { "## Remova $ver ($date)" } else { "## Remova $ver" }
} else {
  $body += $lines[$start]
}
# Guard the slice: an empty section would make `..` count downwards and re-emit the heading.
if ($end - $start -gt 1) { $body += $lines[($start + 1)..($end - 1)] }

$text = ($body -join "`n").Trim()
if (-not $text) {
  Write-Error "CHANGELOG '## [$ver]' section is empty - refusing to publish an empty Release body."
}

$outDir = ".release-notes"
if (-not (Test-Path $outDir)) { $null = New-Item -ItemType Directory -Path $outDir }
$outFile = Join-Path $outDir "$Tag.md"
Set-Content -Path $outFile -Value $text -Encoding utf8 -NoNewline

Write-Host "Release body written: $outFile ($(($text -split "`n").Count) lines)"
exit 0
