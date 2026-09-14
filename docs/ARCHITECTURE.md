# Remova 架构说明

> Tauri 2 + React 19 + Rust（Windows）。本文描述后端模块、命令线程模型、数据流与安全设计。  
> 对应源码：`src-tauri/src/`，命令注册见 `lib.rs::run()`。

---

## 1. 技术栈

| 层 | 技术 |
|---|---|
| Shell | Tauri 2（`remova` crate，lib name `remova_lib`） |
| UI | React 19 + TypeScript + Vite；i18n 为 `src/i18n.ts` 字典 |
| 后端 | Rust 2021，`windows` crate 0.58（Registry / WinRT / GDI / Shell / Restore） |
| 额外依赖 | `serde` / `serde_json` / `png`（图标 PNG 编码） |
| 辅助 bin | `src/bin/list_apps.rs`（CLI 列表对比） |

---

## 2. 模块地图（`src-tauri/src`）

```
lib.rs          Tauri 命令层 + base64 编码；invoke_handler 注册
main.rs         入口，调用 remova_lib::run()
apps.rs         已安装软件枚举（Uninstall 注册表 + Store 合并）
storeapps.rs    WinRT PackageManager 枚举 MSIX/Store 包
scanner.rs      关联扫描（CleanupItem / ScanResult / 证据评分）
executor.rs     卸载命令解析、dry-run、真删流水线
safety.rs       统一安全门禁：is_safe_fs / is_safe_to_delete_registry
backup.rs       备份会话创建、文件复制、reg export
restore.rs      会话列表与还原（path_map + reg import）
regscan.rs      只读注册表助手（子键/值/DWORD/BINARY/默认值）
regops.rs       写侧注册表（删键/删值/写 binary/service Start/sc/schtasks）
manage.rs       启动项 / 服务 / 计划任务列表与启用禁用
sysops.rs       重启删、系统还原点、UAC 提权重启
installmon.rs   安装前后快照差分
orphans.rs      孤儿目录扫描
dirsize.rs      目录体积估算（可取消）
icon.rs         DisplayIcon → PNG data URL
ignore.rs       忽略列表 JSON
history.rs      清理历史 JSONL
```

### 2.1 职责要点

| 模块 | 关键类型 / 函数 | 说明 |
|---|---|---|
| `apps` | `InstalledApp`, `scan_installed_apps` | 三个 Uninstall 源 + `storeapps::scan_store_apps`；过滤 KB / Update；解析 InstallDate、DisplayIcon |
| `storeapps` | `scan_store_apps` | `PackageManager.FindPackagesByUserSecurityId("")`；`uninstall_string = remova-store:<FullName>`；屏蔽框架/系统包前缀 |
| `scanner` | `CleanupItem`, `analyze_associations` | 证据加权 → score → confidence/risk；score &lt; 30 丢弃 |
| `executor` | `run_cleanup_dry`, `run_full_cleanup`, `build_uninstall_command` | dry-run 只校验；full：备份 → 卸载器 → 逐项删 |
| `safety` | `is_safe_fs`, `is_safe_to_delete_registry`, `critical_service_names` | 扫描与执行共用 |
| `backup` / `restore` | `create_session`, `backup_item`, `restore_session` | 见 §5 |
| `manage` | `set_startup_enabled` 等 | StartupApproved 设计见 §7 |
| `installmon` | `begin` / `end` → `MonitorDiff` | 预算 80k 路径；差分取前 200 文件 / 100 注册表项 |
| `orphans` | `scan_orphans` | 与已安装列表做路径/名称排除；上限 80 条 |
| `dirsize` | `walk_size_kb` + `AtomicBool` 取消 | BFS，跳过符号链接 |

---

## 3. Tauri 命令与线程模型

约定：

- **async + `spawn_blocking`**：会阻塞的 IO / 注册表 / 扫描 / 删除。不阻塞 webview 事件循环。
- **sync**：纯读、本地缓存、极轻操作，直接在命令线程执行。

### 3.1 列表与图标

| 命令 | 线程 | 返回 |
|---|---|---|
| `list_installed_apps` | async / blocking | `Vec<InstalledApp>` |
| `app_icon_data` | async / blocking | `Option<String>`（`data:image/png;base64,...`） |
| `begin_size_estimate` | sync | `()` 清除取消标志 |
| `estimate_dir_size_kb` | async / blocking | `i64`（KB） |
| `cancel_size_estimate` | sync | `()` 置取消标志 |

### 3.2 分析与清理

| 命令 | 线程 | 返回 |
|---|---|---|
| `analyze_associations` | async / blocking | `ScanResult` |
| `run_cleanup_dry_run` | async / blocking | `CleanupReport` |
| `run_full_cleanup` | async / blocking | `FullCleanupReport`（内含 history.append） |

### 3.3 备份 / 还原 / 历史

| 命令 | 线程 | 返回 |
|---|---|---|
| `restore_latest_backup` | async / blocking | `Vec<String>` 消息 |
| `list_restore_sessions` | sync | `Vec<String>` 会话名 |
| `restore_session_by_name` | async / blocking | `Vec<String>` |
| `list_cleanup_history` | sync | `Vec<HistoryEntry>`（最多 200） |
| `export_history_csv` | sync | `String` CSV（最多 500 行） |

### 3.4 系统 / 权限

| 命令 | 线程 | 返回 |
|---|---|---|
| `is_elevated` | sync | `bool`（OpenProcessToken + TokenElevation） |
| `disk_usage` | sync | `DiskInfo { free_gb, total_gb }`（SystemDrive） |
| `elevate_restart` | sync | `Result<(), String>`（ShellExecuteW runas） |

### 3.5 管理（启动项 / 服务 / 任务）

| 命令 | 线程 | 返回 |
|---|---|---|
| `list_startup_items` | async / blocking | `Vec<ManageItem>` |
| `list_services` | async / blocking | `Vec<ManageItem>` |
| `list_scheduled_tasks` | async / blocking | `Vec<ManageItem>` |
| `set_startup_enabled` | async / blocking | `()` |
| `set_service_start_disabled` | async / blocking | `()` |
| `set_task_enabled` | async / blocking | `()` |

`ManageItem`: `{ name, detail, location, enabled }`。启动项 `location` 形如 `HKCU\...\Run::ValueName`。

### 3.6 杂项工具

| 命令 | 线程 | 返回 |
|---|---|---|
| `register_context_menu` / `unregister_context_menu` | sync | `()` |
| `load_ignore` / `ignore_publisher` / `ignore_app_name` | sync | `IgnoreList` |
| `scan_orphan_leftovers` | async / blocking | `Vec<CleanupItem>` |
| `begin_install_monitor` | async / blocking | `()` |
| `end_install_monitor` | async / blocking | `MonitorDiff` |

---

## 4. CleanupItem 数据流

### 4.1 类型

```ts
// src/types.ts ↔ scanner.rs
CleanupItem {
  path: string
  kind: "file" | "dir" | "registry"
  score: number            // clamp(-200, 120)
  confidence: "confirmed" | "suspected"
  risk: "low" | "medium" | "high"
  reason: string
  evidence: Evidence[]     // { code, label, weight, detail }
}
```

阈值：`SCORE_CONFIRMED = 90`，`SCORE_SUSPECTED_MIN = 30`。

| score | confidence | risk |
|---|---|---|
| ≥ 90 | confirmed | low |
| 30–89 | suspected | medium |
| &lt; 30 | （不产出） | — |
| 服务 / 计划任务 | suspected | **high（强制）** |

### 4.2 生成路径

```
UI 选中 InstalledApp
    │ invoke analyze_associations { app }
    ▼
lib.rs → spawn_blocking
    ▼
scanner::analyze_associations(name, install_location, publisher, registry_key)
    │ slugify(name) → name_slugs
    │ extract_exe_stems(install_dir)
    │
    ├─ 1 安装目录          score 90  confirmed  low
    ├─ 2 产品同名目录      40 exact / 30 fuzzy
    ├─ 3 Uninstall 键      90  confirmed
    ├─ 4 App Paths         50  confirmed（默认值指向安装目录）
    ├─ 5 Run 值            55 指向安装目录 / 40 名称匹配
    ├─ 6 Services          40  suspected high
    ├─ 7 Scheduled tasks   45  suspected high
    ├─ 8 SOFTWARE\{slug}   40  suspected
    ├─ 9 快捷方式          50  confirmed
    └─ 9 TEMP              30  suspected medium
    ▼
ScanResult { app_name, items[] }
    ▼
UI 默认 selectedPaths = confirmed ∧ risk ≠ high
    ▼
用户调整 → 仅预览 或 清理选中项
```

### 4.3 执行路径

```
run_full_cleanup(app, items, options)
  options.dry_run?
    yes → run_cleanup_dry → CleanupReport（不删）
    no  →
      items 空? → aborted
      backup_enabled?
        yes → create_restore_point（尽力）
              create_session + backup_items
              fail>0 → aborted
        no  → 跳过
      skip_official_uninstall?
        no → build_uninstall_command → spawn + 最长 5min wait
      for item:
        registry? → is_safe_to_delete_registry
                    服务路径 → sc stop/delete
                    任务路径 → schtasks /delete
                    key|value → delete_value else delete_key
        file/dir  → is_safe_fs
                    remove_dir_all / remove_file
                    失败 → schedule_delete_on_reboot
    ▼
FullCleanupReport → history::append（非 dry_run）
```

### 4.4 卸载命令解析（`build_uninstall_command`）

优先级：

1. `prefer_quiet` 且存在 `QuietUninstallString` → 用静默串
2. 前缀 `remova-store:` → `powershell.exe -NoProfile -NonInteractive -Command Remove-AppxPackage -Package '<full>' -ErrorAction Stop`
3. 含 GUID 且含 `msiexec` 或以 `{` 开头 → `msiexec.exe /x {GUID} /qn /norestart`
4. 引号 exe + 参数 → 拆 argv
5. 否则按 Windows 引号规则 `split_win_args`

---

## 5. 备份会话布局

根目录：`%PROGRAMDATA%\Remova\Backup\`

```
{unix_ts}_{safe_app_name}/          # safe: 字母数字与 -_ . 空格，其余 → _，最长 60
├── files/
│   ├── {fnv64}_{original_name}    # 文件副本，或整目录递归复制
│   └── path_map.json              # { "fnv64_name": "C:\\原路径", ... }
└── registry/
    └── {safe_name_of_item_path}/  # \ → __，去掉 : * ? " < > |，最长 180
        ├── export.reg             # reg export /y（/reg:32 或 /reg:64）
        └── value.txt              # 仅当 path 含 |（Run 值）时写入完整 path
```

要点：

- **文件**：`md5_short` 实为 FNV-1a 64（非加密），仅用于命名去重；`path_map.json` 是 BTreeMap，restore 用它把 `files/<rel>` 写回原路径。
- **注册表**：`key|ValueName` 项导出**父键**（`export.reg`），并在 `value.txt` 记录 `key|ValueName`，便于还原时定位；整键项直接导出该键。
- **视图**：路径含 `HKLM32` 或 `WOW6432Node` → `reg export /reg:32`，否则 `/reg:64`。
- **hive 映射**：`HKLM64|HKLM32|HKLM` → `HKLM`；`HKCU` → `HKCU`。
- **中止条件**：`backup_items` 任一项 fail&gt;0 → `run_full_cleanup` 返回 `aborted=true`，不执行删除。

还原（`restore::restore_session`）：

1. 读 `path_map.json`，目录递归拷回，文件 `fs::copy` 回原路径（会覆盖）。
2. 遍历 `registry/*/export.reg`，`reg import`；任一失败立即 Err。
3. 返回消息列表。

历史：`%PROGRAMDATA%\Remova\history.jsonl`，一行一个 JSON `HistoryEntry`。

---

## 6. 三层安全门禁

| 层 | 位置 | 行为 |
|---|---|---|
| **L1 扫描产出** | `scanner::analyze_associations` + `push_item` | 生成前调用 `is_safe_to_delete_registry` / `is_safe_fs`；score &lt; 30 丢弃；服务/任务强制 high；卸载根、Run 根、关键服务直接不产出 |
| **L2 预览 / UI** | `run_cleanup_dry` + React 默认勾选 | dry-run 对每项再跑门禁，失败标 `skipped`；UI 只默认勾选 confirmed ∧ risk≠high；真删前有 confirm 对话框 |
| **L3 执行删除** | `run_full_cleanup` 循环 | 删除**前一刻**再校验：registry → `is_safe_to_delete_registry`，fs → `is_safe_fs`；失败计入 skipped，不碰磁盘/注册表 |

补充硬门禁：

- 备份失败 → 整次 aborted（不进入 L3）
- 关键服务名单（16 个）在 `safety::critical_service_names`，扫描、删除、manage 禁用全部拒绝
- Store 系统包前缀黑名单在 `storeapps::is_blocked_package`，根本不进列表

---

## 7. StartupApproved 禁用设计

**问题**：Windows 登录时会读取所有 `Run` 值；仅重命名或改数据不能可靠禁用。Explorer 用 `StartupApproved\Run` 的二进制标志决定是否执行。

**实现**（`manage.rs`）：

```
location = "{run_key}::{value_name}"

set_startup_enabled(location, enabled)
  1. 拆 key / vname；若 vname 以 .remova-disabled 结尾
     → regops::rename_reg_value 改回原名（兼容旧版）
  2. write_startup_approved(key, base, enabled)
```

**键映射**：

| Run 键 | StartupApproved 键 |
|---|---|
| `HKCU\...\CurrentVersion\Run` | `HKCU\...\CurrentVersion\Explorer\StartupApproved\Run` |
| `HKLM64\...\CurrentVersion\Run` | `HKLM64\...\CurrentVersion\Explorer\StartupApproved\Run` |
| `HKLM32\...\CurrentVersion\Run` | `HKLM32\...\CurrentVersion\Explorer\StartupApproved\Run` |
| `*...\RunOnce` | **无配套键 → no-op，值保持原样** |

**二进制布局**：12 字节缓冲；`buf[0] = 0x02` 启用 / `0x03` 禁用；其余 0。经 `regops::write_reg_binary` 以 `reg add /t REG_BINARY` 写入。

**读侧**（`startup_approved_enabled`）：读同映射键下同名 value；`b[0]==0x03` → 禁用；缺失或其它 → 启用。列表显示时剥掉 `.remova-disabled` 后缀。

---

## 8. 统一 `is_safe_fs` 规则

定义：`safety::is_safe_fs(p: &Path) -> bool`  
调用方：`scanner`（薄封装）、`executor`（dry-run 与真删）。

规则（全部 AND，任一失败即 false）：

1. **路径规范化**：`/` → `\`，转小写；`trim_end_matches('\\')`。
2. **拒绝盘根**：长度为 2 且以 `:` 结尾（`c:` / `c:\` / `d:\`）。
3. **拒绝过浅路径**：`Path::components().count() < 4`。Windows 上即至少 `drive:\dir\file-or-dir`；`C:\foo` 拒绝，`C:\Program Files\MyApp` 允许。
4. **拒绝保护前缀**（等于或以其为前缀）：

   - `c:\windows`
   - `c:\windows.old`
   - `c:\programdata\microsoft`
   - `c:\program files\windowsapps`
   - `c:\program files\common files\microsoft shared`
   - `c:\program files (x86)\common files\microsoft shared`
   - `c:\users\default`

**姊妹门禁** `is_safe_to_delete_registry`（更细，按树段白名单）：

- 归一化 `HKLM64\` / `HKLM32\` → `HKLM\`
- Uninstall / App Paths：根键保护，子键放行
- Services：仅顶层子键；根保护；关键服务名单拒绝
- TaskCache\Tree：根保护；`Microsoft\*` 拒绝；其它厂商树放行
- Run / RunOnce：仅 `key|ValueName` 形式放行；裸根键拒绝
- 其余命中保护前缀：`HKLM\SYSTEM`、`HARDWARE`、`SAM`、`SECURITY`、`SOFTWARE\Microsoft\Windows`、`...\CurrentVersion`、`CRYPTOGRAPHY`、对应 HKCU 等

---

## 9. 前端要点

- 单页 `src/App.tsx`；命令全部经 `@tauri-apps/api/core` 的 `invoke`。
- 图标缓存：`Map<displayIcon, dataUrl|null>` + inflight 去重。
- 占用估算：2 个并发 worker，路径去重，`begin_size_estimate` → 逐个 `estimate_dir_size_kb` → 可 `cancel_size_estimate`。
- 主题：CSS 变量 + `localStorage.remova_theme`。
- 语言：`src/i18n.ts` 字典（zh / en）。
- 关窗保护：batching 或 dryRunning 时 `beforeunload` 拦截；Tauri `onCloseRequested` 在 busy 时 confirm。
- 版本检查：前端请求 GitHub Releases latest。

---

## 10. 数据落盘位置汇总

| 路径 | 内容 |
|---|---|
| `%PROGRAMDATA%\Remova\Backup\<ts>_<app>\` | 备份会话 |
| `%PROGRAMDATA%\Remova\history.jsonl` | 清理历史 |
| `%PROGRAMDATA%\Remova\ignore.json` | 忽略列表 |
| `%PROGRAMDATA%\Remova\monitor_snapshot.json` | 安装监控快照（临时） |
| `HKCU\Software\Classes\*\shell\RemovaDeepUninstall` | 右键菜单 |

---

## 11. 测试

```powershell
cd src-tauri
cargo test --lib
```

覆盖：安全门禁、slugify / 评分、卸载命令解析、dry-run 计数、备份/还原回环、CSV 解析、服务过滤、目录体积、PNG 签名等。  
Store / 列表扫描的端到端测试依赖本机 Windows 环境。
