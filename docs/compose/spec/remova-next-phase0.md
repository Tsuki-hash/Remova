---
feature: remova-next-phase0
status: delivered
updated: 2026-09-11
branch: main
commits: 4927353 (local git, no remote)
---

# Remova-next Phase 0 — Tauri 骨架 + 只读软件列表

## Report

**What was built** — 新仓库 `Remova-next`：Tauri 2 + React 19 + TS + Rust。`list_installed_apps` 枚举 HKLM64/HKLM32/HKCU Uninstall；`safety` 模块与 Python 护栏对齐；React 搜索列表页。

**Verification** — `cargo test --lib` **10/10 PASS**；`npm run build` 成功。

**Journey log** — windows 0.58 `REG_SAM_FLAGS` 不能与 `u32` 直接 `|`；HKLM32 使用无 WOW6432Node 路径 + `KEY_WOW64_32KEY`。

## [S1] Problem

原 Remova 为 Python + CustomTkinter。决定整仓重写为 **Tauri 2 + React 19 + TypeScript + Rust**，项目落在 `D:\Agent-Project\XiaomiMiMoProjects\Remova-next`。第一轮只做可运行骨架与只读「已安装软件列表」，作为与 Python 版 `scan_installed_apps` 的验收基线。

## [S2] Design

1. **结构**
   - `src-tauri/`：Rust 后端，Tauri commands
   - `src/`：React 19 + Vite + TS 前端
2. **Rust `list_installed_apps`**
   - 枚举 HKLM64 / HKLM32(KEY_WOW64_32KEY) / HKCU 的 `...\Uninstall`
   - 字段：name, version, publisher, install_location, uninstall_string, quiet_uninstall_string, source, registry_key, estimated_size_kb
   - 与 Python 版同构别名：`HKLM64\SOFTWARE\Microsoft\...`、`HKLM32\SOFTWARE\Microsoft\...` + 32 视图（无 WOW6432Node 路径）
   - 过滤：无 DisplayName 或无 UninstallString；跳过 KB/Update 模式（可配置）
3. **Safety crate 模块**（只读阶段先落路径/注册表黑名单，供后续删除复用）
   - `protected_dir_prefixes` / `critical_service_names` / `is_safe_to_delete_registry` 的 Rust 版 + 单测
4. **IPC**
   - `#[tauri::command] fn list_installed_apps() -> Result<Vec<InstalledApp>, String>`
5. **UI**
   - 一页列表：名称 / 版本 / 发布者 / 路径 / 来源；搜索过滤；加载状态
6. **验收**
   - `cargo test` 护栏用例通过
   - `npm run build` / `cargo check` 通过（若本机无完整 Tauri 依赖则记录阻塞）
   - 可选：`cargo run` 拉起窗口

## [S3] Out of Scope

删除/备份、关联扫描、UAC 提权自动化、自动更新、i18n、深色主题（后续 Phase）。

## Tasks

- [ ] T1: 工具链与目录骨架 (covers: S2.1)
- [ ] T2: Rust list_installed_apps + safety 单测 (covers: S2.2–S2.3)
- [ ] T3: Tauri command + React 列表页 (covers: S2.4–S2.5)
- [ ] T4: 构建/测试验收 (covers: S2.6)
