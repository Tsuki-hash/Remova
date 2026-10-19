#!/usr/bin/env pwsh
# check-workflows-enabled.ps1 — fail when a repo workflow that gates releases is disabled.
#
# R-R6-04's sibling: check-commands.ps1 proved that a gate reading a gitignored file
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

# Every workflow that must be live for a release to be trustworthy. `paths` is the
# file the workflow is defined in, so a rename is caught too.
$required = @(
  @{ Name = "CI";      Paths = @(".github/workflows/ci.yml") }
  @{ Name = "Release"; Paths = @(".github/workflows/release.yml") }
)

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
  Write-Host "SKIP: gh CLI not found; cannot verify workflow state."
  exit 0
}
$auth = & gh auth status 2>&1
if ($LASTEXITCODE -ne 0) {
  Write-Host "SKIP: gh not authenticated; cannot verify workflow state."
  exit 0
}

# `gh api` resolves the {owner}/{repo} placeholders from the current directory and
# takes no --repo flag, so an explicit repo has to be substituted into the path.
$endpoint = if ($Repo) { "repos/$Repo/actions/workflows" } else { "repos/{owner}/{repo}/actions/workflows" }

$raw = & gh api $endpoint
if ($LASTEXITCODE -ne 0) {
  # Keep the message to one line: gh's own usage text is 100+ lines and buries the cause.
  $first = (($raw | Out-String) -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -First 1)
  Write-Host "SKIP: could not list workflows ($first)"
  exit 0
}
$workflows = ($raw | Out-String | ConvertFrom-Json).workflows

$disabled = @()
$missing = @()
foreach ($want in $required) {
  $found = @($workflows | Where-Object { $_.name -eq $want.Name })
  if ($found.Count -eq 0) {
    # A workflow absent remotely usually means the file was never pushed; report it
    # rather than treating "no workflow" as "nothing to check".
    $missing += $want.Name
    continue
  }
  foreach ($w in $found) {
    if ($w.state -ne "active") { $disabled += "$($w.Name) ($($w.state))" }
  }
}

# A local workflow file with no remote counterpart is the same defect seen from the
# other side: the gate looks configured but will never run.
foreach ($want in $required) {
  foreach ($rel in $want.Paths) {
    if (Test-Path $rel) {
      $remote = @($workflows | Where-Object { $_.path -ieq $rel })
      if ($remote.Count -eq 0) { $missing += "$rel (not registered remotely)" }
    }
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

Write-Host "OK: CI and Release workflows are active ($($workflows.Count) workflows on the remote)."
exit 0
