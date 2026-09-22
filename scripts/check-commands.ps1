# Compare Tauri generate_handler! commands against the code-internal inventory
# (src/lib/api.ts invoke names) and, when present, docs/ARCHITECTURE.md §3.
# R-R6-04: the gate always runs against api.ts — it never silently SKIPs when docs/ is absent.
# Usage: pwsh -NoProfile -File scripts/check-commands.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PsscriptRoot
$lib = Join-Path $root "src-tauri\src\lib.rs"
$api = Join-Path $root "src\lib\api.ts"
$arch = Join-Path $root "docs\ARCHITECTURE.md"

if (-not (Test-Path $lib)) { Write-Error "missing $lib" }
if (-not (Test-Path $api)) { Write-Error "missing $api" }

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

# Code-internal inventory: every command the frontend actually invokes (src/lib/api.ts).
$apiText = Get-Content $api -Raw
$apiCmds = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
foreach ($m in [regex]::Matches($apiText, 'invoke(?:<[^>]*>)?\s*\(\s*"([a-z][a-z0-9_]+)"')) {
  $null = $apiCmds.Add($m.Groups[1].Value)
}

# Backend-only handlers that api.ts does not call (kept explicit so the check stays honest).
$backendOnly = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
foreach ($n in @('list_restore_sessions')) { $null = $backendOnly.Add($n) }

Write-Host "generate_handler commands: $($codeCmds.Count)"
Write-Host "api.ts invoke commands: $($apiCmds.Count)"

$missingHandler = @($apiCmds | Where-Object { -not $codeCmds.Contains($_) } | Sort-Object)
$unknownHandler = @($codeCmds | Where-Object { -not $apiCmds.Contains($_) -and -not $backendOnly.Contains($_) } | Sort-Object)

if ($missingHandler.Count -gt 0) {
  Write-Host "api.ts invokes commands not registered in generate_handler!:"
  $missingHandler | ForEach-Object { Write-Host "  - $_" }
}
if ($unknownHandler.Count -gt 0) {
  Write-Host "generate_handler! commands not used by api.ts (add to `$backendOnly or wire up):"
  $unknownHandler | ForEach-Object { Write-Host "  - $_" }
}
if ($missingHandler.Count -gt 0 -or $unknownHandler.Count -gt 0) {
  Write-Error "command inventory mismatch (missing_handler=$($missingHandler.Count) unknown_handler=$($unknownHandler.Count))."
}

# Optional cross-check against ARCHITECTURE §3 when the local docs tree is present.
if (-not (Test-Path $arch)) {
  Write-Host "NOTE: docs/ARCHITECTURE.md is not present (docs tree is local-only) — skipped doc table cross-check only. api.ts inventory check already passed."
} else {
  $archText = Get-Content $arch -Raw
  if ($archText -notmatch '(?s)## 3\.[\s\S]*?(## 4\.|## CleanupItem)') {
    Write-Error "ARCHITECTURE.md §3 command tables not found"
  }
  $sec3 = $Matches[0]
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
  Write-Host "OK: ARCHITECTURE §3 matches generate_handler! ($($codeCmds.Count) commands)."
}

Write-Host "OK: command inventory matches generate_handler! ($($codeCmds.Count) commands)."
