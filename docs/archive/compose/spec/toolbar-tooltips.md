---
feature: toolbar-tooltips
status: delivered
updated: 2026-09-14
branch: main
commits: fd6cd9e..pending
---

# Toolbar Tooltips & Remove List Hover Strip

## Report

**What was built** — Removed the 1.1.1 list-row hover/selected detail strip that shifted layout and covered the sticky table header. Secondary tools toolbar buttons now have bilingual native `title` tooltips. List rows expose publisher, path, uninstall string, and registry key via multi-line `title` without layout impact.

**Verification** — `npm run build` (tsc -b && vite build) PASS. Independent review: no critical findings.

**Journey log**
- User feedback: wanted tooltips on the tools toolbar (red-boxed row), not the list detail strip; video showed strip covering table header.
- Decision: native `title` only; work on `main`.

## [S1] Problem
1. 用户需要「更多工具」栏的悬停说明，而不是列表行上方的详情条。
2. 1.1.1 引入的列表 hover/selected 详情条会挤动布局并遮挡 sticky 表头。

## [S2] Design
- 删除 App.tsx 中 `(hoverApp || selected)` 的上方详情条，以及 `hoverApp` 状态与 `onMouseEnter/Leave`。
- 列表行 `title`：prettyAppName、发布者、安装路径、卸载串、注册表键。
- 「更多工具」及各次要工具按钮增加中英文 `title`；主工具栏「更多工具」也补上。
- i18n 增加各工具提示键；已有 `forceCleanHint`/`adminHint`/`selectRowHint` 复用。

## [S3] Out of Scope
- 不改工具栏信息架构、不改列表列、不 bump 版本；commit 需用户确认。

## Tasks
- [x] T1: 移除 hover 详情条与 hoverApp 状态 — acceptance: 悬停行不再出现上方详情条，表头不被遮挡 (covers: S2)
- [x] T2: 行 title + 工具栏 title + i18n — acceptance: 悬停工具按钮可见中文/英文说明；悬停行可见完整路径等 (covers: S2; depends: T1)
- [x] T3: npm run build 通过 — acceptance: 无 TS/构建错误 (covers: S2; depends: T2)
