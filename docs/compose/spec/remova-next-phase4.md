---
feature: remova-next-phase4
status: in-progress
updated: 2026-09-11
branch: main
commits: # filled at delivery
---

# Remova-next Phase 4 — 还原 / 历史 / 官方卸载 / CI

## Report

## [S1] Problem

Phase 3 有备份与删除但无还原、无历史、真清理强制跳过官方卸载、无 CI。

## [S2] Design

1. **restore**：读 session `files/path_map.json` 拷回；`reg import` 恢复 .reg
2. **history**：`%PROGRAMDATA%\Remova\history.jsonl` 追加；UI 历史面板
3. **UI**：选项「调用官方卸载器」；「从最近备份还原」；历史页
4. **CI**：GitHub Actions（Windows：cargo test + npm build）
5. **验收**：cargo test；npm build

## [S3] Out of Scope

还原点、重启删除、i18n 完整英文、安装器。

## Tasks

- [ ] T1: restore + history rust (covers: S2.1–S2.2)
- [ ] T2: UI 开关与历史/还原 (covers: S2.3)
- [ ] T3: CI + README (covers: S2.4)
- [ ] T4: 验收提交 (covers: S2.5)
