---
feature: p01-store-apps
status: delivered
updated: 2026-09-11
branch: main
commits: uncommitted-on-5bc54b3
---

# P0-1 Store / UWP App List

## Report

**What was built** — Installed-app list now merges Microsoft Store / MSIX packages for the current user via WinRT `PackageManager::FindPackagesByUserSecurityId("")` (empty user = current; `FindPackages()` needs admin). System apps under `Windows\SystemApps`, GUID package names, framework/runtime packages (`VCLibs`, `WinAppRuntime`, `WindowsStore`, etc.) are filtered out. Entries use `source=Store`, `registry_key=Store\<PackageFullName>`, and `uninstall_string=remova-store:<PackageFullName>`, which maps to `powershell Remove-AppxPackage` in the executor. Installed date is taken from package install time when available.

**Verification** — `cargo test --lib` PASS (42 tests). `list_apps` on this machine: Store=93 after filters (from 125 raw current-user packages).

**Journey log**

1. `FindPackages()` returns access denied without admin — use current-user API.
2. Unfiltered packages include SystemApps GUIDs — path/name filters required.
3. WinRT init (`RoInitialize`) needed for console/tests; Tauri already initializes.

## [S1] Problem

Classic Uninstall registry enumeration misses Microsoft Store / MSIX apps (PRODUCT-GAPS P0-1).

## [S2] Design

- New `storeapps.rs`; `scan_installed_apps()` extends the registry scan with store packages.
- Blocklist prefixes + SystemApps path + GUID names + WinAppRuntime.
- Uninstall: `remova-store:` token → PowerShell `Remove-AppxPackage` (official uninstall path).
- UI: `来源` column shows `Store`; no new frontend required.

## [S3] Out of Scope

- All-users / provisioned package management
- Per-package capability editor
- Resolving `ms-resource:` display names beyond package Id.Name

## Tasks

- [x] T1: store enumeration + filters + merge into list — acceptance: list_apps includes Store source; tests pass
- [x] T2: executor maps remova-store to Remove-AppxPackage — acceptance: unit test
- [x] T3: PRODUCT-GAPS P0-1 checked
