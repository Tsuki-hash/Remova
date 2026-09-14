---
feature: ui-refresh
status: delivered
updated: 2026-09-11
branch: main
commits: uncommitted-on-4949ff6
---

# UI Refresh: layout, table readability, action hierarchy

## Report

**What was built** — Collapsed secondary tools behind 「更多工具」; primary actions (search, batch, analyze, admin) stay visible. Added status chips (count/admin/disk/estimate/monitor). Table shows pretty publisher (CN= extract), shortened Store GUID names, ellipsis + title tooltips, source badges, tabular numeric sizes. Global focus-visible, disabled, hover, row-hover via injected CSS. Refined light/dark tokens (surface-2, accent-ink, row-hover, shadow).

**Verification** — `npm run build` PASS.

**Reasoning**

1. **IA**: ~12 equal-weight buttons caused decision fatigue; primary path is search → select → analyze/cleanup.
2. **Publisher CN=**: raw DNs are unreadable; CN is the human name, GUID CNs labeled “Signed package”.
3. **Ellipsis + title**: keeps density without losing detail on hover.
4. **Focus rings / disabled opacity**: keyboard a11y and clearer affordance.
5. **Collapsible tools**: preserves power features without crowding.

## [S1] Problem
Toolbar noise, Store GUID/cert names, weak hierarchy, poor table scanability.

## [S2] Design
See Report; implementation in `App.tsx` (`applyTheme`, `prettyPublisher`, `prettyAppName`, `shortPath`, layout).

## [S3] Out of Scope
Full component library / CSS modules split.

## Tasks
- [x] T1 tokens + global a11y CSS
- [x] T2 header chips + primary toolbar + tools drawer
- [x] T3 table pretty fields
- [x] T4 build verify
