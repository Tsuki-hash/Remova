# Remova-next vs Python Remova 对齐清单

| 能力 | Python (Trash) | Remova-next | 备注 |
|---|:---:|:---:|---|
| 已安装软件列表 | ✅ | ✅ | HKLM64/32 + HKCU |
| 列表排序 | ✅ | ✅ | 名称/发布者/路径 |
| 搜索 | ✅ | ✅ | |
| 深度分析证据链 | ✅ | ✅ | 安装目录/AppData/卸载键/App Paths/Run |
| 服务扫描 | ✅ | ✅ | HIGH 疑似展示 |
| 计划任务扫描 | ✅ | ✅ | HIGH 疑似展示 |
| App Paths 归属校验 | ✅ | ✅ | 无安装路径不产出 |
| dry-run 清理 | ✅ | ✅ | |
| 备份 + 失败中止 | ✅ | ✅ | |
| 真删除 | ✅ | ✅ | UI 确认 + safety |
| 官方卸载器 | ✅ | ✅ | 开关 |
| 跳过官方卸载 | ✅ | ✅ | |
| 备份还原 | ✅ | ✅ | 最近会话 |
| 清理历史 | ✅ | ✅ | |
| 历史 CSV | ✅ | ✅ | |
| 管理员状态 | ✅ | ✅ | 只读显示 |
| 关联度星级 | ✅ | ✅ | |
| 深浅色主题 | ✅ | ✅ | CSS 变量 + localStorage |
| 中英切换 | ✅ | ✅ | i18n 字典 |
| 批量卸载 | ✅ | ✅ | 多选队列清理确定项 |
| 磁盘空间条 | ✅ | ✅ | C: 可用/总量 |
| 首次引导 | ✅ | ✅ | localStorage |
| 扫描 ETA | ✅ | ✅ | 分析完成后显示耗时 |
| 版本检查 | ✅ | ✅ | GitHub latest（前端） |
| 优雅退出等待 | ✅ | 🟡 | 任务中 beforeunload 提示 |
| MoveFileEx 重启删除 | ✅ | ✅ | 删除失败时调度 |
| 系统还原点 | ✅ | ✅ | SRSetRestorePointW（尽力） |
| 提权重启 | ✅ | ✅ | ShellExecuteW runas |
| PyInstaller/exe | ✅ | ✅ | v0.1.0 Release：exe + msi + nsis |
| CI | ✅ | ✅ | windows test+build |
