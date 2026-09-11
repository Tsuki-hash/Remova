---
feature: remova-next-phase2
status: delivered
updated: 2026-09-11
branch: main
commits: pending-commit
---

# Remova-next Phase 2 — dry-run 清理链路

## Report

**What was built** — `build_uninstall_command`（MSI/引号/quiet）；`run_cleanup_dry` 仅规划不删除；IPC + React「演练清理」摘要。

**Verification** — cargo test 20/20；npm build OK。

**Journey log** — dry-run 对文件系统要求 `exists()`，注册表走 `is_safe_to_delete_registry`。

## Report

## [S1] Problem

只读分析后无法演练清理。需要与 Python 对齐的卸载命令解析 + **dry-run** 执行（不删除），为后续真删除铺路。

## [S2] Design

1. **Rust `executor`**
   - `build_uninstall_command`：MSI GUID / 引号路径 / quiet 优先（与 Python 测试用例对齐）
   - `run_cleanup_dry(items)`：对选中项走 safety 校验，输出 `CleanupReport { deleted_planned, skipped, errors, item_details }`，**不调用** MoveFileEx / DeleteKey / Uninstall
   - 后续 Phase 再接真删除
2. **IPC** `run_cleanup_dry_run(app, items)`
3. **UI** 预览页「演练清理（dry-run）」按钮 + 结果摘要
4. **验收**：`cargo test` 命令解析用例；npm build

## [S3] Out of Scope

真实删除、备份、还原点、批量。

## Tasks

- [x] T1: build_uninstall_command + tests (covers: S2.1)
- [x] T2: run_cleanup_dry + IPC (covers: S2.1–S2.2)
- [x] T3: React 演练按钮 (covers: S2.3)
- [x] T4: cargo/npm 验收 (covers: S2.4)
