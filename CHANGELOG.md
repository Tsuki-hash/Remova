# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.1.1] - 2026-09-21

Stability, safety and release-engineering fixes on top of 1.1.0. No new features.

### Fixed
- **Analyze / AI-explain / verify race**: a slower scan for a previously selected app can no longer
  overwrite the current app's leftover list, checkbox set, or AI notes; the scan spinner is cleared
  only by the newest request
- Leftover list scroll alignment: software rows are now measured dynamically instead of assuming a
  44 px row, and the scan list no longer clips rows taller than 72 px
- Software list rendering: stable row callbacks plus memoized scan-UI/controller action bags stop the
  whole page re-rendering every ~80 ms during size estimation
- `lastReport` is derived from `report` in one reducer instead of being written from four places, so
  the More-page HTML export can no longer describe a different app than the one on screen
- Size-estimate footer spinner can no longer stick on "estimating" after a refresh, and its pending
  flush timer is cancelled on unmount
- Uninstall command lines with doubled quotes (`""`) or escaped quotes (`\"`) are split with Windows
  `CommandLineToArgvW` rules instead of a naive quote toggle
- HTML report language and column labels follow the UI locale instead of hardcoding Chinese; route
  loading fallback no longer says "Estimating sizes"; the leftover confidence column header no longer
  reuses the "Confirm" button label

### Security
- Anything that deletes a file or directory now goes through a delete-grade gate that also rejects the
  user-data and sync-conflict red lines, so a future caller cannot reach `is_safe_fs` alone
- AI API key is never persisted when DPAPI encryption fails — the save now errors instead of silently
  writing plaintext
- Orphan-flow path association uses the delete-grade gate; shortcut scan roots come from `ProgramData`
  instead of a hardcoded `C:\ProgramData`
- AI explanations are no longer attached by sanitized path alone: an ambiguous or already-consumed
  match is skipped rather than guessed

### Changed
- CI: `push` restricted to `main` with a `concurrency` group that cancels superseded runs, explicit
  `permissions: contents: read`, and `cargo test --workspace` so bin targets are covered
- Release: a missing bundle artifact now fails the workflow instead of publishing an asset-less Release
- `check-versions` additionally asserts the `docs/README.md` marker is present and that the newest
  CHANGELOG section equals the package version, and takes `-ExpectedVersion` as a real parameter
- `gen_icons.py` takes the source image (and optional icons dir) as arguments instead of hardcoded
  machine paths; `smoke-dist.ps1` guards `.Count` for Windows PowerShell compatibility
- Dead frontend surface removed (`stateRef` copies, `removeDoneKeys`/`clearMulti` and their reducer
  arms, Copilot filter duplicates, deprecated `AiNlIntent`, unused `useAppFilter` parameter) and IPC
  results are narrowed with type guards instead of `as` casts
- Shared Windows string helpers (`to_wide`, `wstring_from_reg_data`) consolidated into `fsutil`;
  update-check URLs centralized in `constants.rs`
- Docs: `cargo test --workspace` in README/README.en/ARCHITECTURE/CONTRIBUTING, PowerShell 7 documented
  as a required tool, stale "1.0.1" labels dropped, ARCHITECTURE state-domain table updated for the
  derived `lastReport` and the delete-grade gate

## [1.1.0] - 2026-09-20

### Highlights
- **Deep-uninstall UX loop**: software row → detail panel → rich confirm → official uninstaller → auto-scan → classify → you confirm → optional backup → cleanup → report
- **System-item semantics**: startup shows enabled/disabled; services split run state vs start type (stop ≠ disable, with `set_service_running` IPC); tasks show last/next run
- **Orphan trust**: expandable judgment evidence, bulk select safe/review only, page-level scan status, single-channel toast
- **AI demoted to capability**: smart filter collapse + model settings wording (not a page hero)
- **Risk tiers** on cleanup / force-clean / orphan confirms; force-clean analyzes first

### Added
- **Deep-uninstall UX loop**: rich uninstall confirm (official → scan → classify → confirm → optional backup); five-stage progress bar (`identify/official/scan/analyze/report`); report close-out line after full cleanup
- Software list density: publisher DN collapsed via `prettyPublisher`; at most one high-signal chip per row
- Right detail panel trust copy: linked-item empty state + deep-uninstall expectation under primary action
- Right detail panel: linked leftover buckets (program files / config / registry / shortcuts / startup) with size or count; deep-uninstall recommendation card; drill-down filters the leftover table by bucket
- **ManageItem status fields** (optional IPC): `kind` / `running` / `start_type` / `source_label` / `last_run` / `next_run` / `path`
- **Service stop/start IPC** (`set_service_running` + `sc start/stop`) — stop ≠ disable start type; confirm dialogs distinguish the two
- Orphan leftovers: expandable judgment evidence (no-owner / exe / multi-file / config / install-root / mtime age); bulk select safe/review; page-level scan progress; single-channel toast
- Risk-tier labels on cleanup / force-clean / orphan confirm dialogs (`maxRiskOf` / `riskTierLabel`); force-clean analyzes first, then confirms
- Toolbox hierarchy: Everyday / Advanced / System & help; selection-required tools jump to the software list; current-app chip
- Smart-filter collapse on software toolbar (AI demoted from page hero to capability) + example chips
- **AI decision layer**: auto cleanup conclusion after scan (rule-first, AI-labeled when configured); conclusion actions (clean suggested / review / why-keep)
- Copilot on the software list (NL plan → filter / analyze / batch with confirm; offline keyword fallback)
- Report fixed narrative + “next step” lines; auto AI report reading when enabled; first-scan optional hint for clearer explanations
- Uninstall mode actions moved into the `⋯` menu (official / deep / force / analyze)
- Leftover `size_kb` on file/dir items from a bounded directory walk; backend `CleanupItem.bucket` classification (frontend prefers server bucket)
- Portable zip artifact on GitHub Release workflow
- Typed frontend API for history / manage / AI intent; AI + shell state hooks (`useAiPanelState`, `useShellState`); shared `fsutil`
- Size-estimate batch generation (stale results discarded after cancel/begin); residual / list-filter state hooks (`useResidualState`, `useListFilterChrome`)
- Update check resolves NSIS setup asset; footer opens installer download
- Explorer context menu / cleanup verify moved into `sysops`
- Shared panel style tokens (`panelShell`, `sectionTitle`, `detailRow`, …)

### Changed
- Cleanup backup is **opt-in**: confirm dialogs offer an unchecked “create safety backup” option; default cleanup/batch/orphan/force-clean paths no longer force `backup_enabled: true`
- AI copy demoted: 「AI 详细说明/助手」→「模型设置 / 智能解释 / 智能筛选」; chips show model ready/not set; 「智能设置」→「详细说明」 with AI explain off the scan toolbar as primary action
- Startup list shows 已启用/已禁用 + source (registry/folder/store/service), not 「运行中」
- Services show run state + start type; tasks show last/next run when available
- Manage list icons neutral gray; blue reserved for action/status/selection
- Analyze/uninstall toasts use `analyze-flow` channel (no stacked scan/done pair)
- README product positioning documents the deep-uninstall loop; toolbar ⓘ guide describes the full path
- Cleanup gate: PATH danger check expands `%VAR%` and is limited to system-shaped trees; Registry/Path leftovers require app association when an installed app is known; optional `cleanup_source` on full cleanup; `..` segments rejected; association ignores client `reason`; Monitor/Orphan cleanup always skips the official uninstaller
- Shared leftovers: Common Files **roots** and `Microsoft Shared` stay hard-blocked; vendor subpaths may be cleaned only when the **vendor directory segment** matches install/name/publisher; the confirm dialog warns when Common Files paths are selected
- Restore point decoupled from backup; backup abort surfaces as failure
- Dry-run counts align with full delete (missing File/Dir and absent PATH segments skipped); PATH probe uses the same registry source as scrub; history records delayed deletes
- Manage: rejects `Microsoft*` service writes; `FOLDER::` only accepts known Startup folders; `MANAGE_LOCK` serializes manage mutations; PATH restore without scope evidence defaults to User only; scheduled-task native delete uses the full TaskCache path
- Cleanup report counts reboot-delayed deletes separately from `deleted`; `sc`/`schtasks` native results recorded
- AppData / WebView name-match folders default to **suspected/medium** (not auto-selected)
- MSI uninstall command only when `msiexec` is present or the string is a bare `{GUID}`
- Scan leftover risk-filter chips show real confirm/keep counts (was hardcoded 0)
- Leftover list virtualized for large scan results; size estimates flush in batches
- Batch cleanup still runs the official uninstaller when no default-selectable leftovers exist
- Version narrative unified at **1.1.0** (skip separate 1.0.1 release)
- **Architecture**: scanner split into `scanner/{mod,fs_scans,reg_scans}`; `policy.rs` façade so dry-run/full share `gate_cleanup_item`; install monitor tighter roots with cache/temp/log diffs demoted; `SoftwarePage` `React.memo` + controller hook; `CategoryId` / `formatSize` single-sourced; App no longer re-exports types
- **Frontend structure**: domain reducers (`src/hooks/reducers/`) for shell / residual / aiPanel / scanUi / listFilter / appCore; MorePage business hooks; scan UI chrome in `useScanUiState`; i18n split `src/i18n/{zh,en,index}.ts`; residual/AI/Shell hooks expose an `actions` API
- **Shell setters** apply real functional updaters (not toggle-on-fn)
- Route-level code-split for Software / Manage / More / Orphan pages; ScanActions `busy` uses a state expression instead of a ref during render
- `Safety IPC`: registry path gates emit `safety:protected::*` codes
- **CI**: npm + rustc dependency caches; `clippy --all-targets`; ESLint flat config + `npm run lint`; `typecheck:tests`; `scripts/check-commands.ps1`; frontend `dist` smoke; release portable zip smoke; version consistency script; Release attaches CHANGELOG body
- **Docs**: PARITY/PRODUCT-GAPS/ACCEPTANCE link paths fixed; acceptance notes historical v0.1.0 + current baseline **1.1.0**; docs index aligned to package version; CHANGELOG Unreleased de-duplicated; ARCHITECTURE restore order + env prefixes + dynamic service count documented
- `.gitignore`: track live docs (`ARCHITECTURE` / `USER-GUIDE` / `product/`); keep `docs/reviews`, `docs/compose`, `docs/archive` private; release notes no longer dump the whole CHANGELOG body
- Portable zip unified via `pwsh` + `scripts/package-portable.ps1`

### Fixed
- Cleanup gate (P0): server-side recompute of `user_data` / sync-conflict / `shared` — forged IPC flags cannot delete Documents/Downloads/etc.; exact user profile red-line roots and `Common Files` roots are skipped even when IPC flags claim otherwise
- Cleanup gate (P0): PATH scrub rejects system entries (`Windows`/`System32`/PowerShell…)
- **PATH leftover Safety Vault**: production backup snapshots PATH segments to `path.json` (no tree copy); restore merges missing segments only
- **Manage IPC safety**: PackagedStartup writes restricted to StartupApproved keys; Run locations must map to known Run/RunOnce keys; `set_task_enabled` rejects `\Microsoft\Windows\*` system tasks
- Expanded critical service names for manage disable/list; `is_safe_fs` protected prefixes include SystemRoot / ProgramData / ProgramFiles / SystemDrive
- **Ignore rules enforced in backend**; registry value restore prefers `value.reg`
- Medium association gate: unrelated filesystem leftovers skipped at delete time (AR-10); name/publisher slug gate keeps fail-closed — min token length 5, generic English stopwords rejected, name-slug hits require an install root when `install_location` is empty
- Orphan flow uses the safety gate only (no fake slug gate)
- dry-run shares delete gates: user_data / shared / ignore / AR-10 association
- `setMulti` / `setSelectedPaths` functional React updaters resolved inside the reducer (no lost concurrent updaters; residual checkbox & batch multi)
- Uninstall / official-uninstall entry points reject re-entry while busy; empty leftover selection cannot confirm cleanup
- Backup only processes items that pass the cleanup gate; skipped items failing backup do not abort; `path_map.json` write failure counts as a backup fail and aborts cleanup; value.reg export failure aborts backup with `backup:value_reg`
- PATH restore takes `PATH_LOCK`; `reg.exe` writes translate `HKLM64`/`HKLM32` hive aliases; PATH read failure no longer falls back to the process `PATH` (structured `path:io` IPC)
- SVC enable/disable uses manual (3)/disabled (4) + `MANAGE_LOCK`; full delete counts already-missing File/Dir as skipped (not deleted)
- Executor skips `shared` leftovers and ignore-list paths at delete time; cleanup and PATH scrub share a process mutex; appCore reducer single-sourced
- Empty leftover paths filtered before the full cleanup abort check
- **RemovaError**: backup/restore/PATH critical paths emit `backup:*` / `restore:*` / `path:io` codes; AI commands surface model/network failures as `ai:*` instead of silent empty results; frontend `formatError` maps them
- FN-04 rescan wired after cleanup; single ErrorBanner on the software page; More-page install-monitor diff panel always visible when `monitorDiff` exists
- Orphan page: default-safe selection + error banner hook; shell/AI hooks return stable identities for memo
- Stable software-list callbacks so `AppRow.memo` is not defeated; MorePage badges use i18n (`badgeNew` / `badgeRunning`)
- Dead frontend components removed (`RelationGraph` / `RelationOverview`); `ManageItem` type lives in `types.ts`; `NlIntent` single type
- Open location: validate path exists, select files in Explorer, fall back if System32 explorer fails, and map missing-path errors to actionable text
- Executor never deletes `user_data` paths (hard skip + report)
- History CSV escapes quotes/commas in all string fields
- `list_backup_sessions` runs on the blocking pool (no longer sync on the command thread); `backup_item` delegates to production `backup_item_with_map`; cleanup backup stage extracted to `try_backup_phase`; shared `fsutil::fnv1a64` with magic numbers centralized in `constants.rs`
- Testing Library + jsdom coverage: confirm store, CleanupConclusion, hooks actions; `batchEngine` covers ok / no-leftover uninstall / failed / cancel / invoke error
- ESLint exhaustive-deps cleaned; lint exits 0
- Public docs index no longer links private `docs/reviews` paths; USER-GUIDE AI entry is 「详细说明」 with conclusion card + Copilot documented
- ARCHITECTURE command table synced with `generate_handler!` (removed ghost `restore_latest_backup`; added AI/verify/backup session commands); PRODUCT-GAPS / ARCHITECTURE / USER-GUIDE aligned with shipped features

### Security
- Cleanup executor enforces the user-data red line even if a path slips past scan-time marking
- AI key stored via DPAPI and never returned to the frontend; cloud payloads pass through `sanitize_path`

[1.1.1]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.1.1
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
