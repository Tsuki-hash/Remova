---
feature: remova-next-parity2
status: delivered
updated: 2026-09-11
branch: main
commits: e60101b
---

# Remova-next 对齐批次 2

## Report

**What was built** �?主题/i18n/磁盘/引导/版本检�?批量清理�?
**Verification** �?cargo 27/27；npm build OK�?
**Journey log** �?i18n 返回类型�?Strings 联合；disk_usage Win32 API�?
## Report

## [S1] Problem

PARITY.md 仍缺：深浅色、中英、磁盘空间、首次引导、版本检查、批量卸载�?
## [S2] Design

1. **主题**：CSS 变量 light/dark + 侧边栏切�?+ localStorage
2. **i18n**：`t()` 字典 zh/en + 切换
3. **磁盘**：`disk_usage` command（GetDiskFreeSpaceExW�?4. **首次引导**：localStorage `remova_guided` + 提示�?5. **版本检�?*：GitHub latest release（可失败�?6. **批量卸载**：多选列�?�?队列 dry-run/清理 confirmed �?
## [S3] Out of Scope

重启删除、还原点、elevate 重启完整链路�?
## Tasks

- [x] T1: 主题 + i18n (covers: S2.1–S2.2)
- [x] T2: 磁盘 + 引导 + 版本 (covers: S2.3–S2.5)
- [x] T3: 批量卸载 (covers: S2.6)
- [x] T4: 测试构建推�?(covers: S2)
