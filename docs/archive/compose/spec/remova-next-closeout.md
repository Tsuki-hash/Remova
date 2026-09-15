---
feature: remova-next-closeout
status: delivered
updated: 2026-09-11
branch: main
commits: 59d9c8b
---

# Remova-next 最终收�?
## Report

**What was built** �?sc/schtasks 删除；指定会话还�?API；list_apps 对比脚本本机 **100% 交集**；ACCEPTANCE 清单�?
**Verification** �?cargo 29/29；compare_lists coverage 100%；npm build OK�?
**Journey log** �?Python winreg �?Rust 32/64 视图路径一致后列表可完全对齐�?
## Report

## [S1] Problem

仍缺：Python↔Rust 列表实测对比、验收清单、任�?服务更干净删除、指定会话还原�?
## [S2] Design

1. **compare_lists.py**：调�?Python `scan_installed_apps` �?Rust `list_installed_apps`（经 cargo run 示例�?tauri 较难）——改�?Python 读注册表 + 独立 Rust bin `list_apps` 输出 JSON 对比
2. **ACCEPTANCE.md**：真机步骤清�?3. **删除**：任务用 `schtasks /delete`；服务尝�?`sc stop/delete` 后仍删注册表
4. **还原**：`list_restore_sessions` + `restore_session_by_path`

## [S3] Out of Scope

签名、官网�?
## Tasks

- [x] T1: list_apps bin + compare 脚本 (covers: S2.1)
- [x] T2: 任务/服务删除命令 (covers: S2.3)
- [x] T3: 指定会话还原 (covers: S2.4)
- [x] T4: ACCEPTANCE + 跑对�?(covers: S2.1–S2.2)
- [x] T5: 测试构建推�?(covers: S2)
