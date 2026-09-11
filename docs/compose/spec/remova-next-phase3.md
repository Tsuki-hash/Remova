---
feature: remova-next-phase3
status: in-progress
updated: 2026-09-11
branch: main
commits: # filled at delivery
---

# Remova-next Phase 3 — 备份与真实清理

## Report

## [S1] Problem

仅有 dry-run。需要与 Python 版语义一致的：备份 → 官方卸载（可选）→ 残留删除；备份失败或磁盘不足则中止。

## [S2] Design

1. **`backup` 模块**
   - 会话目录：`%PROGRAMDATA%\Remova\Backup\<ts>_<app>\`
   - 文件/目录 `copy`；注册表 `reg export /reg:32|64`（或 winreg 枚举 JSON 简化为 reg.exe）
   - `fail_n>0` → 中止
2. **`executor::run_full_cleanup(app, items, dry_run, skip_official, backup_enabled)`**
   - dry_run=true：Phase 2 逻辑
   - false：备份 → 官方卸载 → 逐项 safety → 删键/删文件
3. **删除**
   - Registry：递归 `DeleteKeyEx`；Run 值 `DeleteValue`
   - FS：`remove_dir_all` / `remove_file`
4. **UI**：确认框 +「备份并卸载」与「dry-run」
5. **验收**：cargo 测试（命令/规划/safety）；npm build；不在测试里真删系统键

## [S3] Out of Scope

还原点 COM、MoveFileEx 重启删除、批量、还原 UI。

## Tasks

- [ ] T1: backup 模块 (covers: S2.1)
- [ ] T2: run_full_cleanup 真删除 (covers: S2.2–S2.3)
- [ ] T3: UI 确认执行 (covers: S2.4)
- [ ] T4: 测试与构建 (covers: S2.5)
