---
feature: remova-next-full-migration
status: in-progress
updated: 2026-09-11
branch: main
commits: # filled at delivery
---

# Remova-next 完整迁移补齐

## Report

## [S1] Problem

v1.0.0 已通过验收，但仍缺完整迁移项：快捷方式/TEMP/Software 键扫描、还原会�?UI、历史过滤、证据详情、清理项勾选、关窗确认�?
## [S2] Design

1. **scanner**：Desktop/Start Menu .lnk；TEMP；`SOFTWARE\Publisher\Product` �?`SOFTWARE\Product`
2. **UI**：预览勾选；证据详情；历史过滤；还原会话下拉；关闭前确认
3. **WINDOW**：on_close_requested 对话框（tauri 事件�?4. **文档**：`MIGRATION.md` 标记完整迁移

## [S3] Out of Scope

公开仓库、代码签名�?
## Tasks

- [x] T1: 扫描 shortcuts/temp/software (covers: S2.1)
- [x] T2: UI 勾�?证据/历史过滤/还原会话 (covers: S2.2)
- [x] T3: 关窗确认 (covers: S2.3)
- [x] T4: 测试构建推�?+ MIGRATION (covers: S2.4)
