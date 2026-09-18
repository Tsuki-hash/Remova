# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed
- **PATH leftover Safety Vault**: production backup now snapshots PATH segments to `path.json` (no silent no-op / accidental directory tree copy)
- PATH restore merges missing segments back into User/Machine PATH (does not overwrite the whole environment)
- **Manage IPC safety**: PackagedStartup writes restricted to StartupApproved keys; Run locations must map to known Run/RunOnce keys
- Expanded critical service names for manage disable/list (WinDefend, Appinfo, DcomLaunch, Power, ProfSvc, …)
- `exportHtmlReport` unit test now exercises the real export function (was a local mock)
- **`is_safe_fs`**: protected prefixes include `SystemRoot` / `ProgramData` / `ProgramFiles` / `SystemDrive` (not only `c:\`)
- **Ignore rules enforced in backend**: list/scan/orphans consume publisher/name/path rules; empty publisher no longer treated as ignored
- **Registry value restore**: Run-style `key|Value` backups write `value.reg`; restore prefers single-value import
- Software list: removed virtualization full-map fallback when the virtual window is empty

### Changed
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
