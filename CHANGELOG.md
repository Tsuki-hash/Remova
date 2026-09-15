# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- In-app confirm dialog (replaces native `window.confirm`); danger actions use hold-to-confirm
- Toast host for success/error/info feedback with auto-dismiss
- After official uninstall succeeds, residual review opens automatically (no extra prompt)

### Changed
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
