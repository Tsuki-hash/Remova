---
feature: beginner-ux
status: delivered
updated: 2026-09-14
branch: main
commits: 60e295b..1d57320
---

# Beginner UX: simplified main flow

## Report

**What was built** — Main list is now four columns (checkbox / name / size / Uninstall). Clicking 「卸载」 confirms, runs only the official uninstaller via new `run_official_uninstall`, then offers a residual scan on success. Residual cleanup always backs up and skips a second official uninstall; deep-analyze (More menu) still has the optional official checkbox. Secondary tools live under a 「更多」 dropdown. Selected-row detail strip shows publisher/version/date/source/path. Light theme remains default; guide copy matches the new flow.

**Verification** — `cargo test --lib` PASS (57, includes `official_uninstall_no_string`); `npm run build` PASS; `npm test` PASS (6). Reviewer: 1 CRITICAL (Admin i18n hardcode) + 2 MAJOR (leftover prompt on failed uninstall; deep-analyze official checkbox removed) fixed and rebuild PASS.

**Journey log**
- Extracted official uninstall from `run_full_cleanup` so beginner path does not also delete residuals.
- Capture `InstalledApp` before `refreshApps` so leftover scan still has registry/path after the entry disappears.
- Reviewer flagged leftover offer on `had_command` — gated to `ok` only to avoid deleting a still-installed app.
- Restored deep-analyze official checkbox after review; post-uninstall path forces skip-official.

## [S1] Problem

Remova still feels like a power-user tool: a dense 8-column table, many equal-weight toolbar buttons, and a multi-step path (select → 深度分析 → 勾选 → 清理) before anything useful happens. New users do not know which button is the real uninstall, what “深度分析” means, or why they must run a dry-run first. The product needs a default path that matches Windows “卸载” expectations while keeping deep residual cleanup available.

## [S2] Design

Settled product decisions:

1. **Simplify the main flow** — default surface is a short list, not a full forensic table.
2. **Primary uninstall path** — row 「卸载」 runs the **official uninstaller** first; residual scan is a follow-up prompt, not a prerequisite.
3. **Minimal list + More menu** — default columns are icon, name, size, Uninstall; secondary tools live under 「更多」.
4. **Workspace** — implement on `main` (user choice).

### 2.1 Main list (default)

| Column | Content |
| --- | --- |
| ✓ | multi-select checkbox (batch still supported) |
| App | icon + pretty name (existing `prettyAppName`) |
| Size | existing size estimate / cache |
| Action | primary 「卸载」 button per row |

- Publisher, version, install date, source, path, uninstall string, registry key move to:
  - row `title` tooltip (already partially present), and
  - a **detail strip** under the toolbar when a row is selected (read-only, 2–3 lines, not a table).
- Sorting stays on name / size only (drop publisher/date/path sort from headers).
- Virtualization stays (`@tanstack/react-virtual`); update `colSpan` and column count.
- Selected-row highlight (sky rail) stays.

### 2.2 Uninstall flow (row button)

```
click 卸载
  → confirm (official uninstaller; name shown)
  → invoke run_official_uninstall(app)
  → busy state 「卸载中…」 (button disabled, row spinner/status)
  → result banner (ok / fail / no uninstall string)
  → if ok or partial: prompt 「扫描残留？」
       yes → analyze_associations(app)  // app object captured pre-uninstall
           → residual panel (simplified)
           → default select: confirmed && risk !== high  (existing rule)
           → 「清理选中」 uses backup_enabled=true, skip_official_uninstall=true
       no  → refresh app list, done
```

- Store apps (`remova-store:`) reuse existing `build_uninstall_command` PowerShell path.
- Official uninstall **does not** delete residuals by itself (new dedicated command).
- Existing 深度分析 path remains available from More menu for power users (scan panel unchanged in capability, restyled lightly).

### 2.3 Residual panel (after uninstall or deep analyze)

Simplified presentation of the same `ScanResult`:

- Header: app name + counts (N items · M confirmed).
- Rows: path (ellipsis), kind badge, risk badge, checkbox. Hide raw score/match/evidence columns by default; evidence stays behind ⓘ.
- Actions: 「跳过」 (back to list + refresh), 「清理选中」 (danger primary), 「仅预览」 optional ghost.
- Default checkbox rule unchanged.

### 2.4 Toolbar & More menu

Primary toolbar:

- Search input
- Source filter (compact select)
- Batch button only when `multi.size > 0` (label: 批量卸载)
- 「更多」 dropdown button

「更多」 menu items (each opens existing panel/action):

- Admin elevate (also show persistent warning chip in header when not admin)
- History / Export CSV / Restore sessions
- Startup · Services · Tasks (Manage panel)
- Force residual clean (selected app)
- Ignore publisher / Ignore app
- Orphan scan
- Install monitor
- Deep analyze (selected app) — power path
- Shell context menu register/unregister
- Open Releases
- Theme toggle / Language toggle

Header keeps: title, app count chip, admin chip, disk chip, estimate/monitor chips when active.

### 2.5 Guided copy

Replace multi-step forensic guide with:

> 选中软件点「卸载」即可。卸载完成后可再扫描并清理残留文件。高级工具在「更多」里。

### 2.6 Backend contract

New Tauri command:

```rust
#[tauri::command]
async fn run_official_uninstall(app: InstalledApp) -> Result<OfficialUninstallResult, String>
```

```rust
pub struct OfficialUninstallResult {
    pub ok: bool,
    pub message: String,
    pub had_command: bool,
}
```

- Extract the official-uninstall spawn/wait block from `run_full_cleanup` into `executor::run_official_uninstall(app) -> OfficialUninstallResult` and call it from both paths so timeout/Store/MSI behavior stays identical.
- `run_full_cleanup` behavior unchanged for batch / existing flows.
- Unit test: `run_official_uninstall` on app with empty uninstall strings returns `had_command=false`, `ok=false`, message `"no uninstall string"` (no process spawn).

### 2.7 Frontend contracts

- New state: `uninstallingKey: string | null`, `officialResult: OfficialUninstallResult | null`, `postUninstallApp: InstalledApp | null`, `askLeftovers: boolean`, `showMoreMenu: boolean`.
- Capture full `InstalledApp` before official uninstall so residual scan still has name/path/publisher/registry_key.
- After residual cleanup or skip, refresh `list_installed_apps`.
- i18n: add zh/en keys (`uninstall`, `uninstalling`, `scanLeftovers`, `leftoversPrompt`, `moreMenu`, `detailStrip`, …). Do not leave English hardcodes on zh UI.
- Keep existing safety: real residual delete still goes through `run_full_cleanup` with confirm + backup.

### 2.8 Visual polish (light default)

- Slightly larger row height (≥40px) and primary Uninstall button contrast.
- Empty / loading / error states use short human copy, not raw `analyze 1.2s · N items` as the only feedback (keep timing in muted secondary text if useful).
- No dark-default regression; light remains default (`remova_theme_v2`).

## [S3] Out of Scope

- Card/grid list layout
- `useRemovaApi` data layer (ARCH-3) and `thiserror` (ARCH-5)
- Changing scan scoring, backup format, or safety gates
- New installers / version bump / GitHub release
- Full CSS-module redesign or component library
- Horizontal marketing-style landing UI inside the desktop app

## Tasks

- [x] T1: Extract `executor::run_official_uninstall` + register `run_official_uninstall` command — acceptance: `cargo test --lib` passes including new empty-string unit test; command listed in `invoke_handler` (covers: S2.6)
- [x] T2: i18n zh/en keys for uninstall flow, residual panel, More menu, guide copy — acceptance: no hardcoded English on primary new controls when lang=zh (covers: S2.2; S2.5)
- [x] T3: Minimal list columns + AppRow Uninstall button + selected detail strip — acceptance: default table shows only ✓ / name / size / action; details visible when row selected or via tooltip (covers: S2.1)
- [x] T4: Uninstall → residual prompt → residual cleanup wiring — acceptance: row Uninstall confirms, calls official command, offers leftover scan, cleanup uses backup + skip official (covers: S2.2; S2.3; S2.7)
- [x] T5: More menu consolidates secondary tools; batch only when selection non-empty — acceptance: primary toolbar ≤ search + filter + batch(if any) + 更多; all previous tools reachable from menu (covers: S2.4)
- [x] T6: Visual polish pass (row height, button hierarchy, guide text, empty/loading) — acceptance: light default intact; guide matches new flow (covers: S2.5; S2.8)
- [x] T7: Verify + review — acceptance: `cargo test --lib` and `npm run build` PASS; reviewer checks spec compliance (covers: all)
