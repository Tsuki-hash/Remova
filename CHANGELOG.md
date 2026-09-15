# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [1.3.0] - 2026-09-16

### Added
- AI orchestration module (`ai.rs`): OpenAI-compatible / Anthropic / Ollama chat, path sanitize, TTL cache
- AI settings panel: provider presets (OpenAI / Anthropic / Ollama), labeled fields, key status, privacy notes
- Residual list “AI explain” button + per-row model summaries (advisory only)
- Cleanup confirm includes optional AI risk brief when enabled
- Shared-runtime heuristic (`shared.rs`): VC++/.NET/WebView/etc. marked and unchecked by default
- Optional AI cleanup-report summary button
- Copilot panel: natural-language intent → plan preview → filter / analyze / batch / force-clean (all with confirm)
- Rule-based ignore suggestions for shared leftover roots (Package Cache, Common Files, …)
- Design doc: `docs/AI-FEATURES.md`
- Dependency: `ureq` (rustls) for backend AI HTTP

### Fixed
- Duplicate software rows when the same product is registered under both HKLM64 and HKLM32
- Row `⋯` menu clipped by virtualized list overflow (portal + fixed positioning)
- Beginner-friendly filters: removed technical source dropdown; chips use 桌面程序 / 商店应用; friendly source labels in details

### Notes
- AI features are **off by default**; core uninstall path is unchanged without configuration
- AI never deletes files — it only explains, ranks, and plans; `safety` gates still apply

## [1.2.1] - 2026-09-16

### Added
- In-app confirm dialog (replaces native `window.confirm`); danger actions use hold-to-confirm
- Toast host for success/error/info feedback with auto-dismiss
- After official uninstall succeeds, residual review opens automatically (no extra prompt)
- More page split into Everyday / Advanced sections with clearer tool rows
- Service/task/startup disable confirmations with risk copy
- Collapsible selected-app detail strip
- Localized running/stopped status labels (replaces ON/OFF)
- Size estimate progress chip (`done/total`)
- Cleanup report summary cards (deleted / failed / skipped) + open backup folder
- `open_path_in_explorer` Tauri command (System32 explorer.exe)
- App list loading skeleton + smarter empty-search copy
- Indeterminate progress bar while deep-analyzing
- Unit tests for `looksMicrosoft` manage filter

### Changed
- Deep analyze removed from software toolbar (row ⋯ menu is the entry)
- More page ignore action focuses on selected publisher
- Force clean / ignore live under Advanced; history/restore/export under Everyday
- Search box uses `useDeferredValue` for smoother typing on large lists
- Manage Microsoft/system filter: path + token heuristics instead of bare "windows" substring
- Status chips use pill radius for a more consumer look
- Restore-point status in report uses success/warn colors
- Confidence labels are human-readable (确定/疑似) instead of star glyphs
- Analyze / batch / monitor / ignore / manage actions report results via toast with localized copy
- Forced-clean empty result no longer reuses "no history" copy
- Residual cleanup after official uninstall always skips a second official uninstaller run

## [1.2.0] - 2026-09-15

### Added
- Left sidebar modules: Software / Startup / Services / Tasks / More
- Category chips on software list (All / Desktop / Store / Large / Recent); Large sorts by size; category persists
- Bottom batch action bar when rows are selected
- Toolbox page (history, restore, export CSV, force clean, orphan scan, install monitor, ignore, context menu, Releases)
- Full pages for startup items, services, and scheduled tasks (virtualized lists)
- Global notice/error chrome on every module
- Row `⋯` menu: deep analyze / force clean / ignore app / ignore publisher
- Startup scan: Run/RunOnce (HKLM64/32+HKCU), Policies Explorer Run, user+common Startup folders
- Service list: skip kernel drivers; show Auto/Manual/Disabled + Description; default hide Microsoft/system
- Task list: positional CSV columns + OEM decode (no more hostname-as-name)

### Changed
- IObit-like light shell: soft blue accent, rounded cards, larger icons
- Sidebar collapses to icon rail under 1100px width; subtitle is Deep Uninstall only
- Orphan scan / monitor → cleanup / deep analyze auto-switch to Software page
- Manage pages default “enabled only”; loading/empty copy
- Comprehensive review doc: `docs/20260915-项目全面评审.md`

## [1.1.1] - 2026-09-14

### Added
- Row-level official uninstall with post-success leftover scan prompt
- `run_official_uninstall` Tauri command (shared with full cleanup)
- More menu for secondary tools (history, restore, manage, force clean, ignore, orphan, monitor, deep analyze, theme/lang)
- Selected-app detail strip (publisher/version/date/source/path)
- Empty-list human copy

### Changed
- Default list is four columns: checkbox / name / size / uninstall
- Beginner guide points to Uninstall then leftover scan
- Residual cleanup after official uninstall always backs up and skips a second official uninstaller
- Batch cleanup runs official uninstaller by default
- Toolbar only shows batch button when rows are selected

## [1.1.0] - 2026-09-14

### Added
- Async Tauri commands for all heavy IO (list/analyze/cleanup/monitor/manage)
- Dry-run result card with planned/skipped counts and item details
- Startup disable via Explorer `StartupApproved\Run` binary flag
- Uninstaller wait (5 min timeout) before residual cleanup
- Run value backup via parent key export
- Unified `is_safe_fs` with drive-root and shallow-path rejection
- SOFTWARE registry keys downgraded to Suspected
- `--analyze` context menu argument parsing and single-instance focus
- System drive detection for disk usage
- Version comparison + timeout on update check
- Expanded orphan-scan skip list (Package Cache, InstallShield, Windows Kits, …)
- i18n for toolbar buttons, batch details, HTML report, close confirm
- CSP with `ipc: http://ipc.localhost` for Tauri 2 production IPC
- Vitest frontend tests + Rust `pipeline_smoke` integration test
- CI: cargo fmt/clippy; release workflow runs vitest
- USER-GUIDE and ARCHITECTURE docs
- App.tsx split into panels + memo `AppRow`
- List virtualization via `@tanstack/react-virtual`
- Backup session management (list, size, delete)
- Install-monitor diff → cleanup list
- Batch cleanup optional official uninstaller
- Source filter dropdown
- Icon disk cache under `%LOCALAPPDATA%\Remova\icons`
- Absolute System32 paths for `sc`/`schtasks`/`reg`
- `schtasks` multi-locale header matching
- Direct `RegQueryValueExW` for string reads
- Single-write `path_map.json` during backup

### Fixed
- Batch cleanup stuck after cancel/empty selection
- HKLM32 registry key deletion (WOW64 view)
- List hover strip covering sticky table header
- `delete_key` parent handle rights for `RegDeleteTreeW`

### Earlier 1.1.0 work (2026-09-11)
- P0–P2 product backlog: Store apps, batch progress, restore UI, manage console,
  force clean, ignore list, orphan scan, HTML report, install monitor, releases link

## [1.0.x]

- Initial Python legacy → Tauri/React/Rust migration
