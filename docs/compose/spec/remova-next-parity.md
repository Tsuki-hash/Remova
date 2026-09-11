---
feature: remova-next-parity
status: in-progress
updated: 2026-09-11
branch: main
commits: # filled at delivery
---

# Remova-next 对齐 Python 剩余项

## Report

## [S1] Problem

与 Python Remova 相比，Rust 版缺少：服务/计划任务扫描、列表排序、磁盘概览、提权入口、历史 CSV、证据星级等。

## [S2] Design

1. **scanner** 增加 services / scheduled tasks（HIGH 疑似，默认不选——列表只读展示）
2. **UI**：列头排序；C: 可用/总量；「请求管理员」（ShellExecute runas 自重启）；关联度星级；历史导出 CSV
3. **对比**：`docs/PARITY.md` 清单

## [S3] Out of Scope

批量卸载队列、完整 i18n、深色主题资源、自动更新。

## Tasks

- [ ] T1: services/tasks 扫描 (covers: S2.1)
- [ ] T2: UI 排序/磁盘/提权/星级 (covers: S2.2)
- [ ] T3: 历史 CSV (covers: S2.2)
- [ ] T4: 测试构建提交 (covers: S2)
