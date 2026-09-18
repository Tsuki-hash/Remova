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
scanner/         关联扫描（mod: types/score/analyze；fs_scans; reg_scans）
shared.rs       共享运行库启发式（VC++/.NET/Common Files…）
executor.rs     卸载命令解析、dry-run、真删流水线（user_data 硬拦）
safety.rs       统一安全门禁：is_safe_fs / is_safe_to_delete_registry
fsutil.rs       共享目录复制 + CSV 转义
backup.rs       备份会话创建、文件复制、reg export
restore.rs      会话列表与还原（path_map + reg import）
regscan.rs      只读注册表助手（子键/值/DWORD/BINARY/默认值）
regops.rs       写侧注册表（删键/删值/写 binary/service Start/sc/schtasks）
manage.rs       启动项 / 服务 / 计划任务列表与启用禁用
sysops.rs       重启删、系统还原点、UAC 提权重启
installmon.rs   安装前后快照差分
orphans.rs      孤儿目录扫描
dirsize.rs      目录体积估算（可取消；限长 walk 供 size_kb）
icon.rs         DisplayIcon → PNG data URL
ignore.rs       忽略列表 JSON + 路径/发布者建议与应用
history.rs      清理历史 JSONL
ai.rs           AI 编排：配置、脱敏、chat、缓存、意图解析（默认关闭）
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
| `shared` | `is_shared_item` | 残留项是否像共享运行库（默认不勾选） |
| `ignore` | `suggest_from_leftovers`, `apply_suggestions` | Common Files / Microsoft Shared 等路径忽略建议（**不含** Package Cache 全局忽略） |
| `ai` | `AiConfig`, `explain_items`, `risk_brief`, `parse_nl_intent` | 默认关闭；不执行删除；超时与缓存见 `ai.rs`；explain **按脱敏 path 回填** |

### 2.2 前端模块地图（`src/`）

```
App.tsx            主壳：状态机 + 列表/扫描切换（目标持续压行）
lib/api.ts         统一 invoke 数据层（History/Manage/AI 已类型化）
lib/decision.ts    唯一勾选谓词 defaultSelectable / chips / 分桶
lib/linkedItems.ts 残留展示分桶（程序文件/配置/注册表…）+ drill-down 过滤
lib/updateCheck.ts GitHub 最新 Release 检测
components/        Shell、AppRow、AppDetailPanel、ReportPanel、Scan/Orphan/More…
hooks/             useSizeEstimate / useAnalyzeFlow / useCleanupHandlers / useAppBoot…
```

**安全谓词单一来源**：是否默认勾选一律 `decision.ts::defaultSelectable`（confirmed && !high && !shared && !user_data）。禁止在组件内复制该条件。

**CleanupItem（1.0.1）**：`path` / `kind`（file|dir|registry|path）/ score / confidence / risk / reason / evidence / shared / user_data / **`size_kb?: Option<u64>`**（仅 file/dir；限长目录求和，超限 None）。

**open_path_in_explorer**：稳定错误码 `open_path:empty|not_found|failed`；文件用 `/select,`；System32 失败回退 PATH explorer。

**linkedItems**：前端 path 启发式把残留归入 programFiles/configFiles/registry/shortcuts/startup/other，用于详情栏展示与过滤，不改变删除语义。

### 2.3 备份路径注入

`backup_root()` 优先读环境变量 `REMOVA_BACKUP_DIR`（测试隔离）；生产默认 `%PROGRAMDATA%\Remova\Backup`。  
`FullCleanupOptions.restore_point`（serde default true）控制是否创建系统还原点——测试必须设 `false`。

---

## 3. Tauri 命令与线程模型

约定：

- **async + `spawn_blocking`**：会阻塞的 IO / 注册表 / 扫描 / 删除。不阻塞 webview 事件循环。
- **sync**：纯读、本地缓存、极轻操作，直接在命令线程执行。
- **唯一源**：命令是否注册以 `src-tauri/src/lib.rs` 的 `tauri::generate_handler![...]` 为准（2026-09-18 回填）。

### 3.1 列表 / 图标 / 更新

| 命令 | 线程 | 返回 |
|---|---|---|
| `list_installed_apps` | async / blocking | `Vec<InstalledApp>`（后端应用 ignore 规则） |
| `app_icon_data` | async / blocking | `Option<String>`（`data:image/png;base64,...`） |
| `begin_size_estimate` | sync | `()` 清除取消标志 |
| `estimate_dir_size_kb` | async / blocking | `i64`（KB） |
| `cancel_size_estimate` | sync | `()` 置取消标志 |
| `check_github_latest` | async / blocking | `Option<LatestReleaseInfo>` |

### 3.2 分析与清理

| 命令 | 线程 | 返回 |
|---|---|---|
| `analyze_associations` | async / blocking | `ScanResult`（path ignore 过滤） |
| `run_cleanup_dry_run` | async / blocking | `CleanupReport` |
| `run_full_cleanup` | async / blocking | `FullCleanupReport`（内含 history.append） |
| `run_official_uninstall` | async / blocking | `OfficialUninstallResult` |

### 3.3 备份 / 还原 / 历史

| 命令 | 线程 | 返回 |
|---|---|---|
| `list_restore_sessions` | sync | `Vec<String>` 会话名 |
| `list_backup_sessions` | sync | `Vec<BackupSession {name,size_kb,created_at}>` |
| `delete_backup_session` | async / blocking | `()` |
| `restore_session_by_name` | async / blocking | `Vec<String>`（文件 path_map；PATH path.json merge；注册表 value.reg 优先，否则 export.reg） |
| `list_cleanup_history` | sync | `Vec<HistoryEntry>`（最多 200） |
| `export_history_csv` | sync | `String` CSV（最多 500 行） |

> 历史文档中的 `restore_latest_backup` **已不存在**，勿再引用。

### 3.4 系统 / 权限 / 路径

| 命令 | 线程 | 返回 |
|---|---|---|
| `is_elevated` | sync | `bool` |
| `disk_usage` | sync | `DiskInfo { free_gb, total_gb }` |
| `elevate_restart` | async / blocking | `Result<(), String>` |
| `open_path_in_explorer` | sync | 稳定错误码 `open_path:empty\|not_found\|failed` |

### 3.5 管理（启动项 / 服务 / 任务）

| 命令 | 线程 | 返回 |
|---|---|---|
| `list_startup_items` | async / blocking | `Vec<ManageItem>` |
| `list_services` | async / blocking | `Vec<ManageItem>` |
| `list_scheduled_tasks` | async / blocking | `Vec<ManageItem>` |
| `set_startup_enabled` | async / blocking | `()`（PACKAGED/Run 键白名单 + critical 服务） |
| `set_service_start_disabled` | async / blocking | `()`（critical 服务拒绝） |
| `set_task_enabled` | async / blocking | `()` |

`ManageItem`: `{ name, detail, location, enabled }`。

### 3.6 忽略 / 孤儿 / 安装监控

| 命令 | 线程 | 返回 |
|---|---|---|
| `load_ignore` / `ignore_publisher` / `ignore_app_name` | sync | `IgnoreList` |
| `suggest_ignore_rules` / `apply_ignore_suggestions` | sync | `Vec<IgnoreSuggestion>` / `IgnoreList` |
| `scan_orphan_leftovers` | async / blocking | `Vec<CleanupItem>`（path ignore 过滤） |
| `verify_cleanup_leftovers` | async / blocking | `Vec<VerifyRow>` |
| `begin_install_monitor` / `end_install_monitor` | async / blocking | `()` / `MonitorDiff` |
| `monitor_diff_to_items` | async / blocking | `Vec<CleanupItem>` |
| `take_pending_analyze` | sync | `Option<String>`（右键菜单待分析路径） |
| `register_context_menu` / `unregister_context_menu` | sync | `()` |

### 3.7 AI（默认关闭，不执行删除）

| 命令 | 线程 | 返回 |
|---|---|---|
| `get_ai_config` / `save_ai_config` | sync | `AiConfigView`（Key 不回传） |
| `ai_risk_brief` | async / blocking | `Option<String>` |
| `ai_explain_items` | async / blocking | `Vec<AiExplainOutput>` |
| `ai_summarize_report` | async / blocking | `Option<String>` |
| `ai_parse_intent` | async / blocking | `AiNlIntent` |

---

## 4. CleanupItem 数据流

### 4.1 类型

```ts
// src/types.ts ↔ scanner/mod.rs
CleanupItem {
  path: string
  kind: "file" | "dir" | "registry" | "path"
  score: number            // clamp(-200, 120)
  confidence: "confirmed" | "suspected"
  risk: "low" | "medium" | "high"
  reason: string
  evidence: Evidence[]     // { code, label, weight, detail }
  shared?: boolean
  user_data?: boolean
  size_kb?: number | null
  bucket?: string | null
}
```

阈值：`SCORE_CONFIRMED = 90`，`SCORE_SUSPECTED_MIN = 30`。

**分类权重（实际决策）**：部分证据类型会**覆盖** score→confidence 表，以产品安全策略为准：

| 产出 | score | confidence | risk | 默认勾选 |
|---|---|---|---|---|
| 安装目录 / Uninstall 键 | 90 | confirmed | low | 是 |
| 产品同名目录 exact / App Paths / 快捷方式 | 40–50 | **confirmed**（路径强关联） | low | 是 |
| AppData / WebView 缓存名匹配 | 40 | **suspected** | medium | 否（需确认） |
| SOFTWARE\{slug} | 40 | suspected | medium | 否 |
| 服务 / 计划任务 / 驱动 | 40–45 | suspected | **high（强制）** | 否 |
| TEMP | 30 | suspected | medium | 否 |

前端默认勾选只看 `confidence/risk/shared/user_data`（`decision.ts::defaultSelectable`），**不直接看 score**。

| score | confidence | risk（默认映射） |
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

根目录：`%PROGRAMDATA%\Remova\Backup\`（或 `REMOVA_BACKUP_DIR`）

```
{unix_ts}_{safe_app_name}/
├── path.json                 # PATH 残留快照（entry + scopes + 当时 User/Machine PATH）
├── files/
│   ├── {fnv64}_{original_name}
│   ├── _path/{safe}.txt      # PATH 条目明文备份
│   └── path_map.json
└── registry/
    └── {safe_name}/
        ├── export.reg        # 父键/整键导出
        ├── value.reg         # 单值导出（Run 类，还原优先）
        └── value.txt         # key|ValueName 元数据
```

**还原顺序**

1. `path_map.json` → 文件/目录写回  
2. `path.json` → **merge** 缺失 PATH 段（不整环境覆盖；User/Machine 按 scopes）  
3. Registry：存在 `value.reg` 则 **单值 import**；否则 `export.reg`  

**门禁**：backup 任一项 fail → cleanup aborted；`path_map` / `value.reg` 写失败计入 fail。

### 6.x AR-10 关联与来源

| 来源 | 关联策略 |
|---|---|
| 正常卸载（有 install_location） | install 前缀 / slug / publisher 启发式 |
| **孤儿（app.source=Orphan 或空 install+registry）** | 仅 `is_safe_fs`，不再用假 slug |
| dry-run | 与真删共用：user_data / shared / ignore / AR-10 / safety |

### 8.x `is_safe_fs` 保护前缀

环境变量生成 + `c:\` 兜底：`SystemRoot`、`SystemRoot.old`、`ProgramData\Microsoft`、`ProgramFiles(x86)\WindowsApps`、`…\Microsoft Shared`、`SystemDrive\Users\Default` 等（见 `safety::protected_fs_prefixes`）。

**关键服务名单**：动态，见 `safety::critical_service_names()`（约 30+，含 WinDefend/Appinfo/DcomLaunch 等；文档勿写死数量）。

**manage 任务写侧**：拒绝 `\Microsoft\Windows\*` 前缀（`manage:protected_task`）。

**IPC 错误**：`RemovaError` 形如 `code::message`（如 `manage:protected::Name`）；前端 `formatError` 兼容旧 `manage:kind:name`。

**模块补全（前端）**：`hooks/reducers/*`、`useMore*`、`src/i18n/{zh,en,index}.ts`（`src/i18n.ts` 为 re-export）、`SoftwarePage`。

---

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
