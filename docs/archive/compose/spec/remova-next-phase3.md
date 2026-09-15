---
feature: remova-next-phase3
status: delivered
updated: 2026-09-11
branch: main
commits: bcf84d3
---

# Remova-next Phase 3 �?备份与真实清�?
## Report

**What was built** �?备份会话（文�?copy + reg export）；`run_full_cleanup` 备份失败即中止；注册�?文件真实删除 + 官方卸载 spawn；UI「备份并清理」需确认�?
**Verification** �?cargo test 24/24；npm build OK�?
**Journey log** �?UI 真清理当�?`skip_official_uninstall=true` 以降低误卸载风险；官方卸载可后续开关�?
# Remova-next Phase 3 �?备份与真实清�?
## Report

## [S1] Problem

仅有 dry-run。需要与 Python 版语义一致的：备�?�?官方卸载（可选）�?残留删除；备份失败或磁盘不足则中止�?
## [S2] Design

1. **`backup` 模块**
   - 会话目录：`%PROGRAMDATA%\Remova\Backup\<ts>_<app>\`
   - 文件/目录 `copy`；注册表 `reg export /reg:32|64`（或 winreg 枚举 JSON 简化为 reg.exe�?   - `fail_n>0` �?中止
2. **`executor::run_full_cleanup(app, items, dry_run, skip_official, backup_enabled)`**
   - dry_run=true：Phase 2 逻辑
   - false：备�?�?官方卸载 �?逐项 safety �?删键/删文�?3. **删除**
   - Registry：递归 `DeleteKeyEx`；Run �?`DeleteValue`
   - FS：`remove_dir_all` / `remove_file`
4. **UI**：确认框 +「备份并卸载」与「dry-run�?5. **验收**：cargo 测试（命�?规划/safety）；npm build；不在测试里真删系统�?
## [S3] Out of Scope

还原�?COM、MoveFileEx 重启删除、批量、还�?UI�?
## Tasks

- [x] T1: backup 模块 (covers: S2.1)
- [x] T2: run_full_cleanup 真删�?(covers: S2.2–S2.3)
- [x] T3: UI 确认执行 (covers: S2.4)
- [x] T4: 测试与构�?(covers: S2.5)
