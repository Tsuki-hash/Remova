# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed
- Cleanup gate (P0): server-side recompute of `user_data` / sync-conflict / `shared` — forged IPC flags cannot delete Documents/Downloads/etc.
- Cleanup gate (P0): PATH scrub rejects system entries (`Windows`/`System32`/PowerShell…)
- `setMulti` functional updates resolved inside the reducer (no lost concurrent updaters)
- Uninstall / official-uninstall entry points reject re-entry while busy; empty leftover selection cannot confirm cleanup
- Backup only processes items that pass the cleanup gate; skipped items failing backup do not abort
- PATH restore takes `PATH_LOCK`; `reg.exe` writes translate `HKLM64`/`HKLM32` hive aliases
- SVC enable/disable uses manual (3)/disabled (4) + `MANAGE_LOCK`; full delete counts already-missing File/Dir as skipped (not deleted)
- Orphan page: default-safe selection + error banner hook; shell/AI hooks return stable identities for memo
- **P0**: functional React updaters for `setMulti` / `setSelectedPaths` (residual checkbox & batch multi)
- **P0**: More-page install-monitor diff panel always visible when `monitorDiff` exists
- **P0**: orphan cleanup AR-10 — orphan flow uses `is_safe_fs` only (no fake slug gate)
- dry-run shares delete gates: user_data / shared / ignore / AR-10 association
- PATH read failure no longer falls back to process `PATH` (structured `path:io` IPC)
- value.reg export failure aborts backup (no silent whole-key restore; `backup:value_reg`)
- manage `set_task_enabled` rejects `\Microsoft\Windows\*` system tasks
- executor skips `shared` leftovers and ignore-list paths at delete time
- cleanup + PATH scrub process mutex; appCore reducer single-sourced
- FN-04 rescan wired after cleanup; single ErrorBanner on software page
- public docs index no longer links private `docs/reviews` paths
- **AR-10 (R2-11)**: name/publisher slug gate keeps fail-closed — min token length 5, generic English stopwords rejected, name-slug hits require install root when `install_location` is empty
- **S-08 smoke**: injectable PATH mock covers backup → `path.json` → scrub → merge restore (no system PATH mutation); value.reg missing export fails backup with structured code
- **RemovaError (R2-10)**: backup/restore/PATH critical paths emit `backup:*` / `restore:*` / `path:io` codes; frontend `formatError` maps them
- **PATH leftover Safety Vault**: production backup snapshots PATH segments to `path.json` (no tree copy); restore merges missing segments only
- **Manage IPC safety**: PackagedStartup writes restricted to StartupApproved keys; Run locations must map to known Run/RunOnce keys
- Expanded critical service names for manage disable/list
- **`is_safe_fs`**: protected prefixes include SystemRoot / ProgramData / ProgramFiles / SystemDrive
- **Ignore rules enforced in backend**; registry value restore prefers `value.reg`
- Medium association gate: unrelated filesystem leftovers skipped at delete time (AR-10)
- AI commands surface model/network failures as `ai:*` instead of silent empty results

### Changed
- CI (engineering): npm + rustc dependency caches; `clippy --all-targets` (lib + tests); frontend `dist` smoke after build; release portable zip smoke (`Remova.exe` / `PORTABLE.txt` / WebView2 / version)
- ARCHITECTURE: path.json/value.reg restore order, is_safe_fs env prefixes, critical service count dynamic
- Release notes no longer dump the entire CHANGELOG body
- Domain reducers / MorePage hooks / i18n split / SoftwarePage
- **Docs (D-08)**: PARITY/PRODUCT-GAPS/ACCEPTANCE link paths fixed; acceptance notes historical v0.1.0 + current baseline **1.1.0**
- **CI (D-09)**: `typecheck:tests` + `scripts/check-commands.ps1` (generate_handler! vs ARCHITECTURE §3); portable zip unified via `pwsh` + `scripts/package-portable.ps1`
- **Shell setters (C-01)**: `setTheme` / `setLangVer` / `setShowDetail` apply real functional updaters (not toggle-on-fn)
- **Manage write-side (S-09/S-08)**: `FOLDER::` only accepts known Startup folders; `MANAGE_LOCK` serializes manage mutations
- **Safety IPC (R2-10)**: registry path gates emit `safety:protected::*` codes
- **ARCHITECTURE**: module map includes `error.rs`/`constants.rs`; critical service count documented as dynamic (~37); `is_safe_fs` env-prefix rules aligned
- **Architecture debt**: `policy.rs` façade (dry-run/full share `gate_cleanup_item`); SoftwarePage `React.memo` + controller hook; `CategoryId` / `formatSize` single-sourced; App no longer re-exports types

- Scan leftover risk-filter chips show real confirm/keep counts (was hardcoded 0)
- Dead frontend components removed (`RelationGraph` / `RelationOverview`); `ManageItem` type lives in `types.ts`
- Cleanup report records `sc delete` / `schtasks delete` native results on service/task items (FN-05)
- AppData / WebView name-match folders default to **suspected/medium** (not auto-selected) (BE-03)
- ARCHITECTURE documents evidence-type confidence overrides vs score table
- `batchEngine` unit tests cover ok / F-1 no-leftover uninstall / failed / cancel / invoke error
- MSI uninstall command only when `msiexec` present or string is a bare `{GUID}` (BE-04)
- `list_backup_sessions` runs on the blocking pool (no longer sync on the command thread)
- `backup_item` delegates to production `backup_item_with_map` (AR-08 partial)
- Backup `path_map.json` write failure counts as backup fail and aborts cleanup (BE-07)
- Shared `fsutil::fnv1a64`; magic numbers centralized in `constants.rs` (BE-05/06)
- Cleanup backup stage extracted to `try_backup_phase` (AR-07 partial)
- Empty leftover paths filtered before full cleanup abort check (AR-10 light)
- **Domain reducers** (`src/hooks/reducers/`): shell / residual / aiPanel / scanUi / listFilter / appCore — App state via `useReducer` actions
- MorePage business hooks: `useMoreHistory` / `useMoreRestore` / `useMoreTools`
- Scan UI chrome in `useScanUiState`; i18n split `src/i18n/{zh,en,index}.ts`
- ESLint exhaustive-deps cleaned; lint exits 0
- **SoftwarePage** extracted from App (FE-02)
- **FN-04**: auto-rescan leftovers after successful official uninstall (default on)
- **PF-08**: route-level code-split for Software / Manage / More / Orphan pages
- ScanActions `busy` uses state expression (`dryRunning||batching||scanning||aiBusy`) instead of `busyRef.current` during render
- Residual/AI/Shell hooks expose `actions` API; App uses them for selection/clear/theme/lang/nav
- **RemovaError** (`error.rs`): structured `code::message` IPC for manage/safety/backup/ai paths; `formatError` maps codes
- Testing Library + jsdom: confirm store, CleanupConclusion, hooks actions unit tests
- Frontend DRY: `lib/aiNarrative` shared by App + ReportPanel; single `AppDetailPanel` instance for list/scan
- Stable software-list callbacks (`useCallback`) so `AppRow.memo` is not defeated
- MorePage badges use i18n (`badgeNew` / `badgeRunning`)
- `NlIntent` single type (AiNlIntent alias); leftover filter uses `isKeepItem`/`isSuggestItem`
- Docs index baseline aligned to package version **1.1.0**; CHANGELOG Unreleased de-duplicated against 1.1.0
- `.gitignore`: track live docs (`ARCHITECTURE` / `USER-GUIDE` / `product/`); keep `docs/reviews`, `docs/compose`, `docs/archive` private
- ARCHITECTURE command table synced with `generate_handler!` (removed ghost `restore_latest_backup`; added AI/verify/backup session commands)
- USER-GUIDE: AI entry is 「详细说明」; conclusion card + Copilot documented; PATH/value restore notes
- CI/Release: version consistency script + frontend lint; Release runs fmt/clippy and attaches CHANGELOG body
- ESLint flat config + `npm run lint`; `typecheck:tests` via `tsconfig.vitest.json`

## [1.1.0] - 2026-09-17

### Added
- **AI decision layer**: auto cleanup conclusion after scan (rule-first, AI-labeled when configured); conclusion actions (clean suggested / review / why-keep)
- Copilot on the software list (NL plan → filter / analyze / batch with confirm; offline keyword fallback)
- Report fixed narrative + “next step” lines; auto AI report reading when enabled
- First-scan optional hint for clearer explanations
- Toolbox redesign: selection-required tools jump to software list; current-app chip
- Right detail panel: linked leftover buckets (program files / config / registry / shortcuts / startup) with size or count
- Deep-uninstall recommendation card; drill-down filters the leftover table by bucket
- Uninstall mode actions moved into the `⋯` menu (official / deep / force / analyze)
- Leftover `size_kb` on file/dir items from a bounded directory walk
- Backend `CleanupItem.bucket` classification (frontend prefers server bucket)
- Portable zip artifact on GitHub Release workflow
- Typed frontend API for history / manage / AI intent
- AI + shell state hooks (`useAiPanelState`, `useShellState`); shared `fsutil`
- Size-estimate batch generation (stale results discarded after cancel/begin)
- Update check resolves NSIS setup asset; footer opens installer download
- Explorer context menu / cleanup verify moved into `sysops`
- Shared panel style tokens (`panelShell`, `sectionTitle`, `detailRow`, …)
- Residual / list-filter state hooks (`useResidualState`, `useListFilterChrome`)

### Changed
- 「智能设置」→「详细说明」；AI explain moved off the scan toolbar as primary action
- Cleanup confirm: compressed risk lines + vault note
- Detail panel stays visible beside the leftover view after analyze
- Leftover list is virtualized for large scan results
- Size estimates flush map updates in batches (fewer re-renders)
- Scanner split into `scanner/{mod,fs_scans,reg_scans}`
- Install monitor: tighter roots; cache/temp/log diffs demoted to suspected
- Analyze/cleanup hooks take grouped `flow` setters
- Batch cleanup still runs official uninstall when no default-selectable leftovers
- Version narrative unified at **1.1.0** (skip separate 1.0.1 release)

### Fixed
- Open location: validate path exists, select files in Explorer, fall back if System32 explorer fails, and map missing-path errors to actionable text
- Executor never deletes `user_data` paths (hard skip + report)
- History CSV escapes quotes/commas in all string fields
- PRODUCT-GAPS / ARCHITECTURE / USER-GUIDE aligned with shipped features

### Security
- Cleanup executor enforces the user-data red line even if a path slips past scan-time marking

[1.1.0]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.1.0

## [1.0.0] - 2026-09-16

First public release.

### Added
- Installed software list with search, size / recent filters, and a fixed detail panel
- Deep uninstall flow: official uninstaller, leftover scan, dry-run, cleanup with Safety Vault backups
- Leftover evidence: confidence, risk, relation overview, and user-data protection (Documents / Downloads / sync never auto-selected)
- PATH cleanup with exact segment matching and environment broadcast
- Orphan leftover page and install monitor for before/after diffs
- Startup / services / scheduled tasks management
- History timeline, backup restore, ignore rules, and optional AI advisory (off by default)
- Custom title bar (no OS chrome) with in-app minimize / maximize / close
- System tray icon; close can minimize to tray or quit (More → 关闭窗口时)
- NSIS installer and portable zip for Windows x64

### Security
- AI API key stored with Windows DPAPI at rest
- Shared-runtime selections require explicit confirmation
- System paths and critical services are blocked from cleanup

[1.0.0]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.0.0
