---
feature: frontend-split
status: in-progress
updated: 2026-09-14
branch: main
commits: 9dd6c40..pending
---

# Frontend Split (App.tsx)

## Report

## [S1] Problem
App.tsx ~1900 行、40+ useState，所有面板内联；改动回归面大（ARCH-2）。

## [S2] Design
- 保持行为不变，只做结构拆分。
- 新文件：
  - `src/components/AppIcon.tsx` — 图标缓存与展示
  - `src/components/HistoryPanel.tsx`
  - `src/components/RestorePanel.tsx`
  - `src/components/MonitorPanel.tsx`
  - `src/components/ManagePanel.tsx`
  - `src/components/BatchSummaryPanel.tsx`
  - `src/lib/format.ts` — prettyAppName/prettyPublisher/shortPath/escapeHtml
  - `src/lib/theme.ts` — loadTheme/applyTheme
- App.tsx 保留：状态编排、主列表、扫描表、工具栏、数据 hook 内联（避免大范围 invoke 重写）。
- 列表虚拟化：本轮不做（需新依赖 + 行高固定假设）；用 memo 化 `AppRow` 降低重渲染。

## [S3] Out of Scope
- 不改 invoke 契约、不加虚拟化依赖、不拆 useRemovaApi 全量 hook。

## Tasks
- [x] T1: 抽 format/theme/AppIcon — acceptance: 新模块存在且可复用 (covers: S2)
- [x] T2: 抽 History/Restore/Monitor/Manage/Batch 面板 — acceptance: 面板组件独立存在 (covers: S2; depends: T1)
- [ ] T3: App.tsx 正式切换到新组件 — acceptance: 行为不变且 build 通过 (covers: S2; depends: T2)
- [ ] T4: memo AppRow — acceptance: 行 props 稳定时不整表重渲染 (covers: S2; depends: T3)
- [ ] T5: review + 报告 — acceptance: review 通过并回填报告 (covers: S2; depends: T4)
