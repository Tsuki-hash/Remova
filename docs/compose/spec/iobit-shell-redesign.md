---
feature: iobit-shell-redesign
status: delivered
updated: 2026-09-14
branch: main
commits: 7f6f587..v1.2.0-tag
---

# IObit-style shell + sidebar modularization

## Report

**What was built** — App shell is now a left nav (软件 / 启动项 / 服务 / 计划任务 / 更多) with IObit-like light tokens (soft blue accent, rounded cards). Startup/services/tasks are full pages via `ManageListPage`; toolbox lives on `MorePage`. Software page keeps official-uninstall → leftover scan, adds category chips and a bottom batch action bar. Global notice/error render on every module; orphan/monitor/analyze handoffs force-navigate to Software.

**Verification** — `cargo test --lib` PASS 57; `npm run build` PASS; `npm test` PASS 6. Reviewer CRITICAL (page-only feedback chrome) and MAJOR (nav switch on orphan/analyze) fixed; leftover `MoreMenu` removed.

**Journey log**
- Hoist notice/error outside page conditionals in multi-page shells — otherwise manage/more tools look dead.
- Persisted nav + analyze/orphan/monitor results must `goNav("software")` or scan UI is invisible.
- Spec inspector right-panel deferred; top detail strip kept for this pass (narrow-window-friendly).

## [S1] Problem

v1.1.1 simplified the uninstall path, but the app still looks like a single dense tool page: every capability (startup/services/tasks/history/restore/monitor) is either a collapsed menu or an overlay panel. Consumers who know IObit Uninstaller expect a left navigation with first-class modules, softer card UI, and less “forensic table” energy. Remova needs a product shell that matches that mental model without changing Rust safety/cleanup contracts.

## [S2] Design

Settled decisions:

1. **Style anchor** — IObit Uninstaller consumer shell (light, blue accent, rounded cards, left nav).
2. **Scope** — frontend shell + IA only; keep existing Tauri commands and uninstall/leftover safety flow.
3. **Sidebar** — five modules: 软件 / 启动项 / 服务 / 计划任务 / 更多.
4. **Workspace** — `main` (user choice).

### 2.1 Layout shell

```
┌────────────┬──────────────────────────────────────────┐
│ Brand      │ Module title · search · status · settings │
│ 软件       ├──────────────────────────────────────────┤
│ 启动项     │                                          │
│ 服务       │           Module content                 │
│ 计划任务   │                                          │
│ 更多       │                                          │
└────────────┴──────────────────────────────────────────┘
```

- App root: `display:flex; height:100vh; overflow:hidden`.
- Sidebar width 200px; collapses to icon rail 64px under 1100px width.
- Active nav item: filled soft-accent pill + left accent bar.
- Header is per-module (title + actions), not one global mega-toolbar.
- Theme/lang/admin stay in header right or More; light remains default.

### 2.2 Module map

| Nav id | Label | Content source |
| --- | --- | --- |
| `software` | 软件 | Existing app list + uninstall + leftover scan + batch + detail strip |
| `startup` | 启动项 | `list_startup_items` / `set_startup_enabled` via Manage data, full-height page |
| `services` | 服务 | `list_services` / `set_service_start_disabled` |
| `tasks` | 计划任务 | `list_scheduled_tasks` / `set_task_enabled` |
| `more` | 更多 | Toolbox cards: 历史 / 备份还原 / 强制清理 / 孤儿扫描 / 安装监控 / 忽略规则 / 右键菜单 / Releases |

- Startup/Services/Tasks become **full pages**, not `ManagePanel` overlay. Reuse `ManageItem` shape and existing toggle commands; drop `showManage` overlay path.
- More page is a card grid of tools; opening History/Restore/Monitor uses a right drawer or in-page section, not modal-on-modal chaos. Prefer in-page section under the card that launched it.
- Software keeps v1.1.1 flow: row Uninstall → official → leftover prompt → residual panel. Deep analyze stays available from Software header overflow or leftover path.

### 2.3 Software page (IObit polish)

- Category chips under search: 全部 / 桌面应用 / Store / 大体积(>200MB) / 最近安装 (filter client-side).
- List rows: larger icon (32px), name, publisher subtitle, size right-aligned, primary Uninstall button.
- Keep virtualization; row min-height ~56px.
- Multi-select batch bar appears at bottom when selection non-empty (IObit-like action bar), not only a toolbar button.
- Selected detail becomes a right inspector panel (280px) instead of top strip when width allows; fallback to top strip when narrow.

### 2.4 Visual tokens (replace “professional dark tool” default)

Light default tokens (dark optional, not default):

| Token | Value | Role |
| --- | --- | --- |
| `--bg` | `#F5F7FA` | app canvas |
| `--surface` | `#FFFFFF` | cards/sidebar |
| `--surface-2` | `#EEF3F9` | subtle fills |
| `--fg` | `#1F2937` | primary text |
| `--muted` | `#6B7280` | secondary |
| `--border` | `#E5E7EB` | hairline |
| `--accent` | `#2F80ED` | IObit-like blue |
| `--accent-soft` | `rgba(47,128,237,.12)` | selection/nav |
| `--danger` | `#EB5757` | uninstall |
| `--ok` | `#27AE60` | success |
| `--warn` | `#F2994A` | caution |
| radius | 10–12px cards, 8px controls | softer consumer feel |
| shadow | `0 1px 2px rgba(16,24,40,.04), 0 8px 24px rgba(16,24,40,.06)` | light elevation |

Type: Segoe UI Variable / Microsoft YaHei UI; data columns keep Cascadia/Consolas tabular nums.

Signature: left nav active pill + software list as card rows (not hairline table chrome).

### 2.5 Frontend structure

```
src/
  App.tsx                 # shell only: nav state, theme, shared dialogs
  components/
    Shell.tsx             # sidebar + header frame
    Sidebar.tsx
    pages/
      SoftwarePage.tsx
      StartupPage.tsx     # thin wrappers over Manage list data
      ServicesPage.tsx
      TasksPage.tsx
      MorePage.tsx
    AppRow.tsx            # keep, restyle
```

- `nav: "software" | "startup" | "services" | "tasks" | "more"` in App state; persist `remova_nav`.
- Manage loaders live in `ManageListPage`; toolbox on `MorePage`.
- i18n: nav labels, category chips, toolbox card titles/descriptions (zh+en parity).

### 2.6 Out of scope

- No new Rust commands unless a page is blocked (expected: none).
- No backend scan algorithm / backup format changes.
- No version bump / release in this feature (separate after user trial).
- No mobile/touch redesign; desktop Tauri window only.
- No fake “health score” unless data already exists (do not invent metrics).

## Tasks

- [x] T1: Token + Shell/Sidebar frame with five nav modules — acceptance: app boots to Software; nav switches pages; light default tokens applied (covers: S2.1; S2.4)
- [x] T2: Software page extract + category chips + batch bottom bar + inspector — acceptance: uninstall/leftover flow still works; virtualization retained (covers: S2.2; S2.3)
- [x] T3: Startup/Services/Tasks full pages using existing manage commands — acceptance: enable/disable works; no Manage overlay required (covers: S2.2)
- [x] T4: More toolbox page (history/restore/force/orphan/monitor/ignore/shell/releases) — acceptance: each tool reachable and functional from More (covers: S2.2)
- [x] T5: i18n zh/en for nav/chips/toolbox — acceptance: zh UI has no hardcoded English on new chrome (covers: S2.5)
- [x] T6: Verify + review — acceptance: `cargo test --lib`, `npm run build`, `npm test` PASS; reviewer checks shell IA and leftover safety (covers: all)
