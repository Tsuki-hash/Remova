---
feature: p1-batch
status: delivered
updated: 2026-09-11
branch: main
commits: uncommitted-on-f82a638
---

# P1 Core: Restore UI, Manage Console, Force Uninstall, Shell Entry

## Report

**What was built** — Completed remaining P1 work on main: (P1-4) session restore picker card; (P1-2) manage console for startup Run values, non-critical services, scheduled tasks with enable/disable; (P1-1) force-clean leftovers button (skip official uninstall, backup, confirmed non-high items); (P1-3) Explorer context menu register/unregister (`HKCU\Software\Classes\*\shell\RemovaDeepUninstall`) and webview drag-drop matching install_location → analyze.

**Verification** — `cargo test --lib` PASS (45 tests). `npm run build` PASS.

**Journey log**

1. `FindPackages` needs admin — already fixed in P0-1; manage list uses registry/schtasks instead of SC Manager APIs.
2. Frontend `L` used before declaration in drag-drop effect — switched to `t().dropHint` and dropped `L` from that effect deps.
3. Context menu uses `reg add` for reliable `Software\Classes\*\shell` creation; second-instance `--analyze` IPC is out of scope (menu opens the app).

## [S1] Problem

PRODUCT-GAPS P1-1..4: restore UI, manage console, force uninstall path, system entry points.

## [S2] Design

- **P1-4**: session list UI + `restore_session_by_name`.
- **P1-2**: `manage.rs` + commands; startup disable renames value with `.remova-disabled`; service disable writes `Start=4` (critical names blocked); task disable via `schtasks /change /disable`.
- **P1-1**: 「强制清理残留」 → analyze + full cleanup with `skip_official_uninstall: true`.
- **P1-3**: register/unregister context menu; drop path prefix-match to `install_location`.

## [S3] Out of Scope
- Service start type beyond disabled/manual
- Single-instance CLI analyze
- Open file location for manage rows (location string is shown)

## Tasks
- [x] T1 manage.rs + commands + tests
- [x] T2 manage panel UI
- [x] T3 force uninstall button + flow
- [x] T4 context menu register + drag-drop
- [x] T5 PRODUCT-GAPS + specs
