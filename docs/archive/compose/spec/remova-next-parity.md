---
feature: remova-next-parity
status: delivered
updated: 2026-09-11
branch: main
commits: a8f097c
---

# Remova-next 对齐 Python 剩余�?
## Report

**What was built** �?服务/计划任务扫描（HIGH 疑似）；列表排序；管理员状态；历史 CSV 导出；证据星级；`docs/PARITY.md` 清单�?
**Verification** �?cargo test 27/27；npm build OK�?
**Journey log** �?TS sort 键须�?`install_location` 与类型一致；提权只读探测 TokenElevation�?
## Report

## [S1] Problem

�?Python Remova 相比，Rust 版缺少：服务/计划任务扫描、列表排序、磁盘概览、提权入口、历�?CSV、证据星级等�?
## [S2] Design

1. **scanner** 增加 services / scheduled tasks（HIGH 疑似，默认不选——列表只读展示）
2. **UI**：列头排序；C: 可用/总量；「请求管理员」（ShellExecute runas 自重启）；关联度星级；历史导�?CSV
3. **对比**：`docs/PARITY.md` 清单

## [S3] Out of Scope

批量卸载队列、完�?i18n、深色主题资源、自动更新�?
## Tasks

- [x] T1: services/tasks 扫描 (covers: S2.1)
- [x] T2: UI 排序/磁盘/提权/星级 (covers: S2.2)
- [x] T3: 历史 CSV (covers: S2.2)
- [x] T4: 测试构建提交 (covers: S2)
