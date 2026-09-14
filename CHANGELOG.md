# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
