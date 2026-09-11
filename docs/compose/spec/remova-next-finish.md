---
feature: remova-next-finish
status: in-progress
updated: 2026-09-11
branch: main
commits: # filled at delivery
---

# Remova-next 收尾（系�?API + 发版�?
## Report

## [S1] Problem

PARITY 剩余：扫�?ETA、优雅退出、MoveFileEx、还原点、提权重启；需产出 exe�?
## [S2] Design

1. **ETA**：analyze 前端计时显示「约�?Ns」（粗估�?2. **MoveFileEx**：删除失败时 `schedule_delete_on_reboot`（占用文件）
3. **还原�?*：`SRSetRestorePointW`（尽力，失败非致命）
4. **提权重启**：`ShellExecuteW runas` 自重�?5. **优雅退�?*：前�?beforeunload 提示
6. **发版**：`tauri build`；tag `v0.1.0`；Release �?exe

## [S3] Out of Scope

签名证书、官网、Freemium�?
## Tasks

- [x] T1: MoveFileEx + 还原�?+ elevate (covers: S2.2–S2.4)
- [x] T2: UI ETA + 退出提�?(covers: S2.1, S2.5)
- [x] T3: tauri build + tag/release (covers: S2.6)
- [x] T4: 验收 (covers: S2)
