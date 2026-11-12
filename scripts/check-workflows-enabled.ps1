#!/usr/bin/env pwsh
# check-workflows-enabled.ps1 — fail when a repo workflow that gates releases is disabled.
#
# Sibling of check-commands.ps1: that gate proved a check reading a gitignored file
# silently skips on a fresh checkout. This one covers the mirror-image failure — a
# gate that is *configured* but *switched off*. CI and Release both sat at
# `disabled_manually` while the README badge showed "no status" and two releases
# (v1.3.0, v1.3.1) shipped with no Release run at all. Nothing in the repo could see
# that state, because the state lives in GitHub, not in the working tree.
#
# Usage:
#   pwsh -NoProfile -File scripts/check-workflows-enabled.ps1          # this repo
#   pwsh -NoProfile -File scripts/check-workflows-enabled.ps1 -Repo owner/name
#
# Requires the GitHub CLI. Skips (with a warning, exit 0) when gh is missing or not
# authenticated, so local runs without a token are not blocked — CI passes GH_TOKEN.
param([string]$Repo = "")

$ErrorActionPreference = "Stop"

# Every workflow required to build and publish a release.
$required = @(
  @{ Name = "CI"; Path = ".github/workflows/ci.yml" }
  @{ Name = "Release"; Path = ".github/workflows/release.yml" }
  @{ Name = "Publish release draft"; Path = ".github/workflows/publish-release.yml" }
)

$mustVerify = $env:CI -ceq 'true' -or -not [string]::IsNullOrWhiteSpace($env:GH_TOKEN)

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
  if ($mustVerify) { throw 'gh CLI is required to verify workflow state in CI.' }
  Write-Host "SKIP: gh CLI not found; cannot verify workflow state."
  exit 0
}
$auth = & gh auth status 2>&1
if ($LASTEXITCODE -ne 0) {
  if ($mustVerify) { throw 'GitHub CLI authentication is required to verify workflow state in CI.' }
  Write-Host "SKIP: gh not authenticated; cannot verify workflow state."
  exit 0
}

# `gh api` resolves the {owner}/{repo} placeholders from the current directory and
# takes no --repo flag, so an explicit repo has to be substituted into the path.
$endpoint = if ($Repo) { "repos/$Repo/actions/workflows" } else { "repos/{owner}/{repo}/actions/workflows" }

$raw = & gh api $endpoint
if ($LASTEXITCODE -ne 0) {
  if ($mustVerify) { throw 'GitHub API failed to list workflows.' }
  Write-Host 'SKIP: GitHub API failed to list workflows.'
  exit 0
}
$workflows = ($raw | Out-String | ConvertFrom-Json).workflows

$disabled = @()
$missing = @()
foreach ($want in $required) {
  $found = @($workflows | Where-Object { $_.name -ceq $want.Name -and $_.path -ceq $want.Path })
  if ($found.Count -ne 1) {
    $missing += "$($want.Name) ($($want.Path))"
    continue
  }
  foreach ($w in $found) {
    if ($w.state -ne "active") { $disabled += "$($w.Name) ($($w.state))" }
  }
}

if ($missing.Count -gt 0) {
  Write-Host "Workflows expected but not registered on the remote:" -ForegroundColor Red
  $missing | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
}
if ($disabled.Count -gt 0) {
  Write-Host "Workflows registered but disabled on the remote:" -ForegroundColor Red
  $disabled | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
  Write-Host "Re-enable with: gh workflow enable <name>" -ForegroundColor Red
}
if ($missing.Count -gt 0 -or $disabled.Count -gt 0) {
  Write-Error "workflow state check failed (disabled=$($disabled.Count) missing=$($missing.Count))."
}

Write-Host 'OK: CI, Release and Publish release draft workflows are active.'
exit 0
