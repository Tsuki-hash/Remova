# Remova 用户指南

> Windows 深度卸载工具。主流程：列表 → 深度分析 → 看懂证据与风险 → 预览 → 清理 → 需要时还原。  
> 界面支持中文 / English 与深色 / 浅色主题（工具栏切换，偏好保存在本地）。

---

## 1. 卸载一个软件（主流程）

按顺序操作即可，不需要额外配置。

### 1.1 打开列表

启动后自动加载已安装软件，来源包括：

| 来源 | 说明 |
|---|---|
| HKLM64 / HKLM32 | 系统级 Uninstall 注册表（64 / 32 位视图） |
| HKCU | 当前用户 Uninstall |
| Store | Microsoft Store / MSIX 包（WinRT PackageManager） |

列表列：名称、版本、发布者、来源、安装路径、占用、安装日期。可按列排序；搜索框按名称 / 发布者 / 路径过滤。

**占用估算**：若注册表 `EstimatedSize` 为 0 或缺失，启动后会自动遍历安装目录估算体积（界面显示 `~xx MB`）。估算可点「停止估算」取消。

**拖放**：把 exe 或安装目录拖进窗口，会自动匹配并开始深度分析。

### 1.2 深度分析

1. 点击列表中的一行选中软件（也可勾选多选框后点「深度分析」，仅在恰好选中 1 个时生效）。
2. 点 **深度分析**。后端只读扫描关联残留，通常几秒完成。
3. 右侧出现清理项列表，每项包含路径、类型（file / dir / registry）、证据与风险分级。

默认勾选策略：**仅勾选「确定」且风险不是「高」的项**。请按需调整。

### 1.3 看懂证据与风险

| 分级 | 含义 | 界面表现 |
|---|---|---|
| ★★★ 确定 | 高置信关联（如官方安装目录、卸载键、指向安装目录的快捷方式） | 默认勾选，风险低 |
| ★★ 疑似 | 名称模糊匹配、TEMP、Software 键等 | 默认不勾，风险中 |
| 服务 / 计划任务 | 命中产品名或安装路径 | **强制高风险，默认不勾** |

点击项旁的 ⓘ 可查看证据明细（code、label、weight、detail）。不确定时先「仅预览」。

**扫描覆盖范围**：

- 安装目录本身
- LOCALAPPDATA / APPDATA / PROGRAMDATA / Program Files / Program Files(x86) 下的产品同名目录
- 卸载注册表键
- App Paths（仅当默认值指向安装目录）
- Run 启动项值（指向安装目录或名称匹配）
- Windows 服务（仅展示，高风险）
- 计划任务 TaskCache\Tree（仅展示，高风险）
- SOFTWARE\Product 注册表键（HKLM64 / HKLM32 / HKCU）
- 桌面 / 开始菜单快捷方式（.lnk / .url / .appref-ms，深度 ≤ 3）
- TEMP 目录中的名称匹配项

### 1.4 仅预览（dry-run）

点 **仅预览**。对已勾选项做安全门禁校验，给出：

- 计划删除数
- 跳过数（未过安全检查）
- 每项状态：`planned` 或 `skipped`

**不会删除任何文件或注册表。** 官方卸载器也不会启动。

### 1.5 清理

1. （可选）勾选「调用官方卸载器」：清理前先跑厂商卸载命令（静默优先；MSI 走 `msiexec /x /qn`；Store 包走 `Remove-AppxPackage`）。
2. 点 **清理选中项**，确认对话框。
3. 后端顺序：
   - 尽力创建系统还原点（失败不阻断）
   - 创建备份会话并备份所有选中项（**任一项备份失败则整次中止，不删任何东西**）
   - 跑官方卸载器（若勾选，最长等待 5 分钟）
   - 逐项真删：注册表先过 `is_safe_to_delete_registry`，文件/目录先过 `is_safe_fs`；服务/任务会先尝试 `sc delete` / `schtasks /delete`
   - 文件被锁定时调度重启后删除（`MoveFileEx` DELAY_UNTIL_REBOOT）
4. 结果面板显示 deleted / failed / skipped，以及 backup 目录路径。

备份位置：`%PROGRAMDATA%\Remova\Backup\<时间戳>_<应用名>\`

### 1.6 还原（需要时）

工具栏展开更多工具 → **还原最近备份**，或在还原对话框中选择指定备份会话 → **确认还原**。

- 文件按 `path_map.json` 写回原路径
- 注册表逐个 `reg import` 对应的 `export.reg`
- 返回每条还原消息

清理历史可在「历史」中查看，支持导出 CSV。

---

## 2. Store 应用

Store / MSIX 包出现在同一列表，来源列显示 `Store`。

- 卸载命令映射为 PowerShell：`Remove-AppxPackage -Package '<PackageFullName>'`
- 系统关键包被过滤，不会出现在列表：`Microsoft.Windows.*`、`VCLibs`、`.NET`、`UI.Xaml`、`WindowsStore`、`DesktopAppInstaller`、WinAppRuntime、GUID 形包名、framework / resource 包，以及安装在 `C:\Windows\SystemApps\` / ImmersiveControlPanel 的包
- 深度分析 / 预览 / 备份清理流程与桌面软件一致；卸载键为 `Store\<PackageFullName>`，安全门禁会拦截非白名单注册表路径

---

## 3. 批量清理

1. 在列表勾选多个软件（左列复选框）。
2. 点 **批量清理**，确认「将对 N 个已选软件依次分析并清理确定项」。
3. 逐个应用：深度分析 → 过滤「确定 + 非高风险」→ 备份 → 清理。
4. 进度显示 `[i/N] 应用名`；可点「取消批量」，**当前应用完成后停止**。
5. 结束后弹出汇总：成功 / 失败 / 无残留跳过，失败项可「重试失败项」。

批量过程中关闭窗口会弹出确认提示。

---

## 4. 强制清理残留

当官方卸载器已跑过、或卸载器损坏但残留仍在时：

1. 选中软件行。
2. 点 **强制清理残留**，确认「将跳过官方卸载器，仅清理已确认的残留项（会先备份）」。
3. 后端自动重新深度分析，只取「确定 + 非高风险」项，以 `skip_official_uninstall: true` + `backup_enabled: true` 执行清理。

适合：控制面板卸载失败后清尾巴；不启动第三方 uninstaller。

---

## 5. 管理：启动项 / 服务 / 计划任务

工具栏 → **管理**，三个页签。

### 5.1 启动项

- 列出 HKLM64 / HKLM32 / HKCU 的 `Run` 与 HKCU `RunOnce`
- 启用状态优先读 Explorer **StartupApproved** 二进制标志（首字节 `0x03` = 禁用）；缺失时回退旧版 `.remova-disabled` 重命名后缀
- **禁用** 写入对应 hive 的 `...\Explorer\StartupApproved\Run`，12 字节缓冲，byte0 = `0x02` 启用 / `0x03` 禁用
- `RunOnce` 无 StartupApproved 配套键，切换为 no-op（一次性项）
- 若值名带 `.remova-disabled`，先改回原名再写标志（迁移旧数据）

### 5.2 服务

- 列出 `HKLM64/32\SYSTEM\CurrentControlSet\Services` 顶层键
- 关键系统服务（eventlog、winmgmt、bits、spooler 等 16 个）不出现、不可操作
- **禁用** 将 `Start` 设为 4；**启用** 设为 3（手动），不会擅自改成自动，避免意外拉起

### 5.3 计划任务

- 解析 `schtasks /query /fo CSV /v`，跳过 `\Microsoft\Windows\` 系统任务
- 启用 / 禁用走 `schtasks /change /tn <名> /enable|/disable`

---

## 6. 孤儿扫描

工具栏 → **孤儿扫描**。

扫描 Program Files、Program Files(x86)、LOCALAPPDATA、PROGRAMDATA 下的顶层目录：

- 排除 Windows / WindowsApps / Common Files / Microsoft / Package Cache / Windows Kits / dotnet 等系统与缓存目录
- 排除能匹配到已安装软件路径或名称的目录
- 只保留「像应用目录」的（含 exe/msi，或至少 3 个文件）
- 输出为「疑似 / 中风险」`CleanupItem`，上限 80 条

结果进入与深度分析相同的勾选 + 预览 + 清理流程。适合清「装过但注册表已丢」的残留目录。

---

## 7. 安装监控

工具栏 → **监控安装**。

1. 点开始：对 Program Files / ProgramFiles(x86) / LOCALAPPDATA / PROGRAMDATA 做文件系统快照（路径预算 8 万条），并快照 Uninstall / Run 相关注册表子键与值名。
2. 去安装目标软件。
3. 点 **结束监控**：再次快照后做差分，展示新增文件（最多 200）与新增注册表项（最多 100）。

状态文件：`%PROGRAMDATA%\Remova\monitor_snapshot.json`（结束后自动删除）。  
无内核驱动、无后台常驻；只对比「有没有多出来」，不跟踪删除或修改。

---

## 8. 忽略列表

选中软件后：

- **忽略发布者**：该发布者所有软件从列表隐藏
- **忽略此软件**：按应用名忽略

规则持久化在 `%PROGRAMDATA%\Remova\ignore.json`（publishers / names / paths）。匹配为大小写不敏感；paths 为前缀匹配（当前 UI 主要提供发布者与名称入口）。

---

## 9. 管理员提权

状态栏显示 **管理员** / **非管理员**。

非管理员时点击提示「清理系统软件可能失败。点击以管理员身份重启」：

- 通过 `ShellExecuteW` + `runas` 以 UAC 提权重启当前 exe
- 失败原因会本地化展示：
  - UAC 拒绝 → 请点「是」，或右键 Remova → 以管理员身份运行
  - 用户取消 → 仍以普通权限运行
  - 找不到可执行文件 → 重新安装或从安装目录启动
  - 其它错误码

清理 HKLM 系统级 Uninstall、服务、计划任务时建议管理员权限。

---

## 10. 主题与语言

| 操作 | 位置 | 持久化 |
|---|---|---|
| 深色 / 浅色 | 工具栏主题按钮 | `localStorage.remova_theme` |
| 中文 / English | 工具栏语言按钮 | 本地语言偏好 |

首次启动会显示引导条：「选中软件 → 深度分析 → 勾选要清理的项 → 清理选中项（会先备份）。不确定时可先仅预览。」点「知道了」后不再出现。

---

## 11. 右键菜单（可选）

工具栏 → **注册右键菜单**：在 `HKCU\Software\Classes\*\shell\RemovaDeepUninstall` 写入菜单项「用 Remova 深度卸载」，命令为 `Remova.exe --analyze "%1"`。  
**取消右键菜单** 删除该键。

---

## 12. 安全须知

1. **真实删除必须经过 UI 确认**，且后端在删除前再次跑安全门禁。
2. 备份是尽力而为，**不能替代系统备份**。
3. 备份会话在 `%PROGRAMDATA%\Remova\Backup\`，可手动归档或删除。
4. 关键注册表前缀（HKLM\SYSTEM、SAM、SECURITY、Windows NT 等）、关键服务名、Microsoft 系统计划任务、磁盘根目录、`C:\Windows` 等路径永远不会被安全门禁放行。
5. 预览（仅预览）永远不删东西；建议首次对陌生软件先预览再清理。
6. 清理历史写入 `%PROGRAMDATA%\Remova\history.jsonl`（dry-run 不写入）。

---

## 13. 常见问题

**列表里没有我要卸载的软件**  
可能是 Store 系统包被过滤、或是「Update for …」/ KBxxxx 系统更新条目。孤儿扫描可处理无卸载记录的残留目录。

**分析很慢**  
首次会遍历多个根目录与注册表，通常数秒。后台占用估算可随时「停止估算」。

**清理后后悔**  
用「还原最近备份」或指定会话还原。服务 / 计划任务删除优先原生命令，注册表键也会备份。

**提示备份失败并中止**  
这是设计行为：任何一项备份失败都不删。检查磁盘空间与 `%PROGRAMDATA%\Remova\Backup` 写权限后重试。

**Store 应用卸载失败**  
部分包需要管理员或被系统占用；可先结束相关进程，或以管理员身份运行 Remova 再试。
