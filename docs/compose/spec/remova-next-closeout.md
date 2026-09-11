---
feature: remova-next-closeout
status: in-progress
updated: 2026-09-11
branch: main
commits: # filled at delivery
---

# Remova-next 最终收口

## Report

## [S1] Problem

仍缺：Python↔Rust 列表实测对比、验收清单、任务/服务更干净删除、指定会话还原。

## [S2] Design

1. **compare_lists.py**：调用 Python `scan_installed_apps` 与 Rust `list_installed_apps`（经 cargo run 示例或 tauri 较难）——改为 Python 读注册表 + 独立 Rust bin `list_apps` 输出 JSON 对比
2. **ACCEPTANCE.md**：真机步骤清单
3. **删除**：任务用 `schtasks /delete`；服务尝试 `sc stop/delete` 后仍删注册表
4. **还原**：`list_restore_sessions` + `restore_session_by_path`

## [S3] Out of Scope

签名、官网。

## Tasks

- [ ] T1: list_apps bin + compare 脚本 (covers: S2.1)
- [ ] T2: 任务/服务删除命令 (covers: S2.3)
- [ ] T3: 指定会话还原 (covers: S2.4)
- [ ] T4: ACCEPTANCE + 跑对比 (covers: S2.1–S2.2)
- [ ] T5: 测试构建推送 (covers: S2)
