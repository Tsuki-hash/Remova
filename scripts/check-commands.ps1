# Compare Tauri generate_handler! commands vs the ARCHITECTURE.md §3 command tables.
# Local-only check: docs/ is no longer committed, so on a CI checkout this exits 0 without verifying.
# Usage: pwsh -NoProfile -File scripts/check-commands.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PsscriptRoot
$lib = Join-Path $root "src-tauri\src\lib.rs"
$arch = Join-Path $root "docs\ARCHITECTURE.md"

if (-not (Test-Path $arch)) {
  Write-Host "SKIP: docs/ARCHITECTURE.md is not present (docs tree is local-only) - nothing to compare."
  exit 0
}
if (-not (Test-Path $lib)) { Write-Error "missing $lib" }

$libText = Get-Content $lib -Raw
if ($libText -notmatch 'generate_handler!\s*\[([\s\S]*?)\]') {
  Write-Error "generate_handler! block not found in lib.rs"
}
$handlerBlock = $Matches[1]
$codeCmds = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
# Handler entries may be bare (`check_github_latest,`) or module-qualified
# (`commands::update::check_github_latest,`) — compare on the final segment.
foreach ($m in [regex]::Matches($handlerBlock, '(?m)^\s*(?:[A-Za-z_][A-Za-z0-9_]*::)*([A-Za-z_][A-Za-z0-9_]*)\s*,?\s*$')) {
  $null = $codeCmds.Add($m.Groups[1].Value)
}

$archText = Get-Content $arch -Raw
if ($archText -notmatch '(?s)## 3\.[\s\S]*?(## 4\.|## CleanupItem)') {
  Write-Error "ARCHITECTURE.md §3 command tables not found"
}
$sec3 = $Matches[0]
# Only first-column command lists: `| `cmd` / `cmd2` | ... |`
$docCmds = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
foreach ($row in ($sec3 -split "`n")) {
  if ($row -notmatch '^\|\s*`') { continue }
  $first = ($row -split '\|')[1]
  foreach ($m in [regex]::Matches($first, '`([a-z][a-z0-9_]+)`')) {
    $null = $docCmds.Add($m.Groups[1].Value)
  }
}

$missingInDocs = @($codeCmds | Where-Object { -not $docCmds.Contains($_) } | Sort-Object)
$ghostInDocs = @($docCmds | Where-Object { -not $codeCmds.Contains($_) } | Sort-Object)

Write-Host "generate_handler commands: $($codeCmds.Count)"
Write-Host "ARCHITECTURE §3 table commands: $($docCmds.Count)"

if ($ghostInDocs.Count -gt 0) {
  Write-Host "Ghost commands in ARCHITECTURE §3 (not in generate_handler!):"
  $ghostInDocs | ForEach-Object { Write-Host "  - $_" }
}

if ($missingInDocs.Count -gt 0) {
  Write-Host "Commands registered but not documented in ARCHITECTURE §3:"
  $missingInDocs | ForEach-Object { Write-Host "  - $_" }
}

if ($ghostInDocs.Count -gt 0 -or $missingInDocs.Count -gt 0) {
  Write-Error "ARCHITECTURE command table mismatch (missing=$($missingInDocs.Count) ghost=$($ghostInDocs.Count))."
}

Write-Host "OK: command table matches generate_handler! ($($codeCmds.Count) commands)."
