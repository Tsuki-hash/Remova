---
feature: p2-enhancements
status: delivered
updated: 2026-09-11
branch: main
commits: uncommitted-on-9551f9c
---

# P2 Enhancements

## Report

**What was built** — Shipped P2-3 ignore list (ProgramData JSON + list filter + ignore publisher/app buttons), P2-5 HTML cleanup report download, P2-2 orphan folder scan into the existing scan preview table, P2-4 Open Releases button (plus existing version notice), P2-1 explicit install monitor (begin/end snapshot of key FS roots + uninstall/Run registry names) with diff panel.

**Verification** — `cargo test --lib` PASS (49 tests). `npm run build` PASS.

**Journey log**

1. Install monitor is user-started snapshots only — no driver (PRODUCT-GAPS 暂缓项).
2. Orphans use path/slug matching against installed list; heuristic app-like dirs (exe or ≥3 files).
3. `installmon` end() type fix: `take_fs_snapshot` returns `FsSnapshot` not nested.

## [S1] Problem
PRODUCT-GAPS P2-1..5.

## [S2] Design
As implemented: ignore.rs, orphans.rs, installmon.rs, HTML export from FullCleanupReport, Releases URL button.

## [S3] Out of Scope
Kernel monitoring; auto-ignore; CI portable packaging changes.

## Tasks
- [x] T1 P2-3 ignore store + list filter + UI
- [x] T2 P2-5 HTML export
- [x] T3 P2-2 orphan scan + UI
- [x] T4 P2-4 release link helpers
- [x] T5 P2-1 install monitor
- [x] T6 PRODUCT-GAPS + verify
