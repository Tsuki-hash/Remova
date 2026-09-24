# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.2.0] - 2026-09-23

工具箱增强与图标统一。相对 1.1.1 为功能版本。

### 新功能

- **清理历史可删了**：历史面板支持删除单条记录，也可一键清空；只动历史日志，不影响备份
- **闲置软件雷达**：找出长期未动、占用偏大的软件（只作建议，卸载仍在软件列表完成）
- **安装包与更新缓存**：清理下载的安装包与更新缓存；每条可「查看位置」，默认不勾选用户库文件
- **磁盘占用雷达**：只读查看哪些目录占空间，可下钻与打开位置
- **专项缓存清理**：开发 / 游戏 / 浏览器工具链缓存（如 npm、Steam 着色器、浏览器 Cache）；不碰存档与浏览器配置

### 体验

- 工具卡、侧栏、管理页签与列表类型图标统一为清晰描边风格
- 任务栏与托盘图标更清晰；软件列表图标在更新后会自动刷新，不再停在旧图
- 清理列表每行可打开所在文件夹

### 修复与安全

- 安装包扫描更克制：不再把普通压缩包或无关 exe 当安装包
- 闲置判定更准：安装很久但最近仍在写入的软件不会被误判为闲置
- 磁盘下钻限制在系统常用目录；专项/安装包清理只允许删除本轮扫描到的路径
- 卸载命令只执行本次扫描登记过的条目，杜绝伪造卸载命令执行任意程序
- 安装路径关联更稳：用户目录等系统浅层根不当成软件目录；`C:\Steam` 这类便携安装根仍可关联
- 还原备份可写回 `Documents\<应用>` 等库子路径（库根 / 开机启动项 / 同步冲突仍禁止）；会话删除按名称可正常操作
- 删除残留时拒绝目录联接/符号链接，避免被中途换路径
- 界面确认支持键盘长按；删除备份前会二次确认
- 清理历史读写加锁，超长日志自动压缩；同内容多条记录可逐条删除；若干错误提示不再暴露系统内部细节

## [1.1.1] - 2026-09-21

稳定性、安全纵深与发布可靠性补丁。**无新功能**，主流程与 1.1.0 相同。

### 亮点

- **分析结果更稳**：快速切换软件时，残留列表、勾选和智能说明不会再被上一次扫描覆盖；导出的 HTML 报告始终对应你正在看的软件
- **大列表更顺**：软件列表与残留列表按实际行高对齐，长列表滚到底不再错位或裁剪；估算体积时操作更跟手
- **删除更稳**：用户库根目录 / 同步冲突永不误删；库内应用数据与 `AppData` 更新包可关联清理（默认需确认）
- **安装包发布更可靠**：安装产物缺失时发布会直接失败，不会再出现「有版本页、没有安装包」

### 行为变化（相对 1.1.0）

删除面更保守的同时，**应用数据仍可清干净**：

1. **用户库「根目录」永不删除**  
   `Documents` / `Downloads` / `桌面` 等库**根本身**以及同步冲突目录，任何删除入口都会拒绝并记为跳过。  
   **库下面的软件子目录**（如 `Documents\某软件\`、`Downloads\安装包.msi`）在与软件关联后**可以清理**，但默认不勾选，需你确认（可能含存档/个人文件）。

2. **Common Files 只认厂商目录段**  
   位于 `Common Files` 下的残留，只有当**厂商目录名**与软件安装名 / 发布者匹配时才会关联可清；任意子串命中不再算关联。`Common Files` 等共享根目录仍然禁止清理。  
   `AppData` 下的缓存、更新安装包（含 updater 目录）**不受此影响**，仍按关联正常清理。

3. **智能说明宁缺毋错**  
   路径脱敏后若无法唯一对应到某条残留，该项不再显示智能说明（而不是显示可能错配的说明）。

### 修复

- 深度分析 / 智能说明 / 清理复核的并发竞态：慢返回不会覆盖当前软件的结果，加载态只由最新请求结束
- 软件列表滚动对齐与残留列表裁剪；体积估算期间整页高频重刷
- 「更多」页导出的 HTML 报告可能描述成另一台软件
- 体积估算角标可能一直停在「估算中」
- 含 `""` 或 `\"` 的卸载命令行拆参错误，官方卸载器可能拉起失败
- HTML 报告语言与表头跟随界面语言；若干加载文案与表头歧义

### 安全

- 删除文件或目录一律经过删除级安全门（含用户数据与同步冲突红线），不依赖调用方自觉
- 系统加密失败时不再明文保存 AI API Key，改为保存失败并提示
- 孤儿清理关联同样走删除级安全门；快捷方式扫描覆盖非 C 盘系统数据目录
- 智能说明回填歧义时跳过，不猜测对应项

### 变更

- 清理报告明细中的跳过原因改为可读说明（用户数据 / 共享组件 / 未关联 / 安全门等）
- 发布正文取自本变更日志的对应版本节；缺节或空节会直接导致发布失败，而不是退回自动生成的提交列表
- 安装包缺失时发布工作流失败，而不是发出没有资产的 Release
- 内部模块整理与测试加固，对外功能接口不变
- 公开仓库不再附带完整文档树；说明以 README、本变更日志与应用内提示为准

## [1.1.0] - 2026-09-20

### Highlights
- **Deep-uninstall UX loop**: software row → detail panel → rich confirm → official uninstaller → auto-scan → classify → you confirm → optional backup → cleanup → report
- **System-item semantics**: startup shows enabled/disabled; services split run state vs start type (stop ≠ disable, with `set_service_running` IPC); tasks show last/next run
- **Orphan trust**: expandable judgment evidence, bulk select safe/review only, page-level scan status, single-channel toast
- **AI demoted to capability**: smart filter collapse + model settings wording (not a page hero)
- **Risk tiers** on cleanup / force-clean / orphan confirms; force-clean analyzes first

### Added
- **Deep-uninstall UX loop**: rich uninstall confirm (official → scan → classify → confirm → optional backup); five-stage progress bar (`identify/official/scan/analyze/report`); report close-out line after full cleanup
- Software list density: publisher DN collapsed via `prettyPublisher`; at most one high-signal chip per row
- Right detail panel trust copy: linked-item empty state + deep-uninstall expectation under primary action
- Right detail panel: linked leftover buckets (program files / config / registry / shortcuts / startup) with size or count; deep-uninstall recommendation card; drill-down filters the leftover table by bucket
- **ManageItem status fields** (optional IPC): `kind` / `running` / `start_type` / `source_label` / `last_run` / `next_run` / `path`
- **Service stop/start IPC** (`set_service_running` + `sc start/stop`) — stop ≠ disable start type; confirm dialogs distinguish the two
- Orphan leftovers: expandable judgment evidence (no-owner / exe / multi-file / config / install-root / mtime age); bulk select safe/review; page-level scan progress; single-channel toast
- Risk-tier labels on cleanup / force-clean / orphan confirm dialogs (`maxRiskOf` / `riskTierLabel`); force-clean analyzes first, then confirms
- Toolbox hierarchy: Everyday / Advanced / System & help; selection-required tools jump to the software list; current-app chip
- Smart-filter collapse on software toolbar (AI demoted from page hero to capability) + example chips
- **AI decision layer**: auto cleanup conclusion after scan (rule-first, AI-labeled when configured); conclusion actions (clean suggested / review / why-keep)
- Copilot on the software list (NL plan → filter / analyze / batch with confirm; offline keyword fallback)
- Report fixed narrative + “next step” lines; auto AI report reading when enabled; first-scan optional hint for clearer explanations
- Uninstall mode actions moved into the `⋯` menu (official / deep / force / analyze)
- Leftover `size_kb` on file/dir items from a bounded directory walk; backend `CleanupItem.bucket` classification (frontend prefers server bucket)
- Portable zip artifact on GitHub Release workflow
- Typed frontend API for history / manage / AI intent; AI + shell state hooks (`useAiPanelState`, `useShellState`); shared `fsutil`
- Size-estimate batch generation (stale results discarded after cancel/begin); residual / list-filter state hooks (`useResidualState`, `useListFilterChrome`)
- Update check resolves NSIS setup asset; footer opens installer download
- Explorer context menu / cleanup verify moved into `sysops`
- Shared panel style tokens (`panelShell`, `sectionTitle`, `detailRow`, …)

### Changed
- Cleanup backup is **opt-in**: confirm dialogs offer an unchecked “create safety backup” option; default cleanup/batch/orphan/force-clean paths no longer force `backup_enabled: true`
- AI copy demoted: 「AI 详细说明/助手」→「模型设置 / 智能解释 / 智能筛选」; chips show model ready/not set; 「智能设置」→「详细说明」 with AI explain off the scan toolbar as primary action
- Startup list shows 已启用/已禁用 + source (registry/folder/store/service), not 「运行中」
- Services show run state + start type; tasks show last/next run when available
- Manage list icons neutral gray; blue reserved for action/status/selection
- Analyze/uninstall toasts use `analyze-flow` channel (no stacked scan/done pair)
- README product positioning documents the deep-uninstall loop; toolbar ⓘ guide describes the full path
- Cleanup gate: PATH danger check expands `%VAR%` and is limited to system-shaped trees; Registry/Path leftovers require app association when an installed app is known; optional `cleanup_source` on full cleanup; `..` segments rejected; association ignores client `reason`; Monitor/Orphan cleanup always skips the official uninstaller
- Shared leftovers: Common Files **roots** and `Microsoft Shared` stay hard-blocked; vendor subpaths may be cleaned only when the **vendor directory segment** matches install/name/publisher; the confirm dialog warns when Common Files paths are selected
- Restore point decoupled from backup; backup abort surfaces as failure
- Dry-run counts align with full delete (missing File/Dir and absent PATH segments skipped); PATH probe uses the same registry source as scrub; history records delayed deletes
- Manage: rejects `Microsoft*` service writes; `FOLDER::` only accepts known Startup folders; `MANAGE_LOCK` serializes manage mutations; PATH restore without scope evidence defaults to User only; scheduled-task native delete uses the full TaskCache path
- Cleanup report counts reboot-delayed deletes separately from `deleted`; `sc`/`schtasks` native results recorded
- AppData / WebView name-match folders default to **suspected/medium** (not auto-selected)
- MSI uninstall command only when `msiexec` is present or the string is a bare `{GUID}`
- Scan leftover risk-filter chips show real confirm/keep counts (was hardcoded 0)
- Leftover list virtualized for large scan results; size estimates flush in batches
- Batch cleanup still runs the official uninstaller when no default-selectable leftovers exist
- Version narrative unified at **1.1.0** (skip separate 1.0.1 release)
- **Architecture**: scanner split into `scanner/{mod,fs_scans,reg_scans}`; `policy.rs` façade so dry-run/full share `gate_cleanup_item`; install monitor tighter roots with cache/temp/log diffs demoted; `SoftwarePage` `React.memo` + controller hook; `CategoryId` / `formatSize` single-sourced; App no longer re-exports types
- **Frontend structure**: domain reducers (`src/hooks/reducers/`) for shell / residual / aiPanel / scanUi / listFilter / appCore; MorePage business hooks; scan UI chrome in `useScanUiState`; i18n split `src/i18n/{zh,en,index}.ts`; residual/AI/Shell hooks expose an `actions` API
- **Shell setters** apply real functional updaters (not toggle-on-fn)
- Route-level code-split for Software / Manage / More / Orphan pages; ScanActions `busy` uses a state expression instead of a ref during render
- `Safety IPC`: registry path gates emit `safety:protected::*` codes
- **CI**: npm + rustc dependency caches; `clippy --all-targets`; ESLint flat config + `npm run lint`; `typecheck:tests`; `scripts/check-commands.ps1`; frontend `dist` smoke; release portable zip smoke; version consistency script; Release attaches CHANGELOG body
- **Docs**: internal planning-doc link paths fixed; acceptance notes historical v0.1.0 + current baseline **1.1.0**; docs index aligned to package version; CHANGELOG Unreleased de-duplicated; ARCHITECTURE restore order + env prefixes + dynamic service count documented
- `.gitignore`: track the user-facing docs; keep working notes and release-planning drafts private; release notes no longer dump the whole CHANGELOG body
- Portable zip unified via `pwsh` + `scripts/package-portable.ps1`

### Fixed
- Cleanup gate (P0): server-side recompute of `user_data` / sync-conflict / `shared` — forged IPC flags cannot delete Documents/Downloads/etc.; exact user profile red-line roots and `Common Files` roots are skipped even when IPC flags claim otherwise
- Cleanup gate (P0): PATH scrub rejects system entries (`Windows`/`System32`/PowerShell…)
- **PATH leftover Safety Vault**: production backup snapshots PATH segments to `path.json` (no tree copy); restore merges missing segments only
- **Manage IPC safety**: PackagedStartup writes restricted to StartupApproved keys; Run locations must map to known Run/RunOnce keys; `set_task_enabled` rejects `\Microsoft\Windows\*` system tasks
- Expanded critical service names for manage disable/list; `is_safe_fs` protected prefixes include SystemRoot / ProgramData / ProgramFiles / SystemDrive
- **Ignore rules enforced in backend**; registry value restore prefers `value.reg`
- Medium association gate: unrelated filesystem leftovers skipped at delete time (AR-10); name/publisher slug gate keeps fail-closed — min token length 5, generic English stopwords rejected, name-slug hits require an install root when `install_location` is empty
- Orphan flow uses the safety gate only (no fake slug gate)
- dry-run shares delete gates: user_data / shared / ignore / AR-10 association
- `setMulti` / `setSelectedPaths` functional React updaters resolved inside the reducer (no lost concurrent updaters; residual checkbox & batch multi)
- Uninstall / official-uninstall entry points reject re-entry while busy; empty leftover selection cannot confirm cleanup
- Backup only processes items that pass the cleanup gate; skipped items failing backup do not abort; `path_map.json` write failure counts as a backup fail and aborts cleanup; value.reg export failure aborts backup with `backup:value_reg`
- PATH restore takes `PATH_LOCK`; `reg.exe` writes translate `HKLM64`/`HKLM32` hive aliases; PATH read failure no longer falls back to the process `PATH` (structured `path:io` IPC)
- SVC enable/disable uses manual (3)/disabled (4) + `MANAGE_LOCK`; full delete counts already-missing File/Dir as skipped (not deleted)
- Executor skips `shared` leftovers and ignore-list paths at delete time; cleanup and PATH scrub share a process mutex; appCore reducer single-sourced
- Empty leftover paths filtered before the full cleanup abort check
- **RemovaError**: backup/restore/PATH critical paths emit `backup:*` / `restore:*` / `path:io` codes; AI commands surface model/network failures as `ai:*` instead of silent empty results; frontend `formatError` maps them
- FN-04 rescan wired after cleanup; single ErrorBanner on the software page; More-page install-monitor diff panel always visible when `monitorDiff` exists
- Orphan page: default-safe selection + error banner hook; shell/AI hooks return stable identities for memo
- Stable software-list callbacks so `AppRow.memo` is not defeated; MorePage badges use i18n (`badgeNew` / `badgeRunning`)
- Dead frontend components removed (`RelationGraph` / `RelationOverview`); `ManageItem` type lives in `types.ts`; `NlIntent` single type
- Open location: validate path exists, select files in Explorer, fall back if System32 explorer fails, and map missing-path errors to actionable text
- Executor never deletes `user_data` paths (hard skip + report)
- History CSV escapes quotes/commas in all string fields
- `list_backup_sessions` runs on the blocking pool (no longer sync on the command thread); `backup_item` delegates to production `backup_item_with_map`; cleanup backup stage extracted to `try_backup_phase`; shared `fsutil::fnv1a64` with magic numbers centralized in `constants.rs`
- Testing Library + jsdom coverage: confirm store, CleanupConclusion, hooks actions; `batchEngine` covers ok / no-leftover uninstall / failed / cancel / invoke error
- ESLint exhaustive-deps cleaned; lint exits 0
- Public docs index no longer links private working-note paths; USER-GUIDE AI entry is 「详细说明」 with conclusion card + Copilot documented
- ARCHITECTURE command table synced with `generate_handler!` (removed ghost `restore_latest_backup`; added AI/verify/backup session commands); ARCHITECTURE / USER-GUIDE aligned with shipped features

### Security
- Cleanup executor enforces the user-data red line even if a path slips past scan-time marking
- AI key stored via DPAPI and never returned to the frontend; cloud payloads pass through `sanitize_path`

## [1.0.0] - 2026-09-16

First public release.

### Added
- Installed software list with search, size / recent filters, and a fixed detail panel
- Deep uninstall flow: official uninstaller, leftover scan, dry-run, cleanup with Safety Vault backups
- Leftover evidence: confidence, risk, relation overview, and user-data protection (Documents / Downloads / sync never auto-selected)
- PATH cleanup with exact segment matching and environment broadcast
- Orphan leftover page and install monitor for before/after diffs
- Startup / services / scheduled tasks management
- History timeline, backup restore, ignore rules, and optional AI advisory (off by default)
- Custom title bar (no OS chrome) with in-app minimize / maximize / close
- System tray icon; close can minimize to tray or quit (More → 关闭窗口时)
- NSIS installer and portable zip for Windows x64

### Security
- AI API key stored with Windows DPAPI at rest
- Shared-runtime selections require explicit confirmation
- System paths and critical services are blocked from cleanup

<!-- R-R6-08: link definitions live at the bottom of the file (Keep a Changelog). -->
[1.1.1]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.1.1
[1.1.0]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.1.0
[1.0.0]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.0.0
