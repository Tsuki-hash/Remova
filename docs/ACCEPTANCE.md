# Remova-next 真机验收清单

安装：Release `v0.1.0` 或 `npm.cmd run tauri dev`。建议以管理员运行。

## A. 只读（无风险）

- [ ] 启动后列表显示约 200+ 软件；与 Python `python main.py --list` 数量接近
- [ ] 搜索、列排序、深浅色、中英切换正常
- [ ] 状态栏显示管理员/磁盘
- [ ] `scripts/compare_lists.py` coverage ≥ 85%

## B. 分析（无删除）

- [ ] 选中带 InstallLocation 的软件 → 深度分析
- [ ] 应出现：安装目录（★★★）+ 卸载注册表键
- [ ] 无安装路径的软件不应出现 App Paths 项

## C. dry-run

- [ ] 演练清理：计划删除数 = 分析确定项；不改系统

## D. 备份清理（高风险——仅测试软件）

- [ ] 创建可丢弃的测试目录 + Run 值，或使用便携软件
- [ ] 备份并清理（勿勾官方卸载器）
- [ ] 确认 `%PROGRAMDATA%\Remova\Backup\` 有会话
- [ ] 确认测试目录/注册表消失或 delayed
- [ ] 历史有记录；导出 CSV

## E. 还原

- [ ] 还原最近备份后测试文件重新出现

## F. 批量

- [ ] 多选两个测试项 → 批量清理，进度 notice 更新

## G. 回归

- [ ] 不误删：服务 Winmgmt 等关键项不在结果或不可删
- [ ] Microsoft 计划任务不出现

通过 A–C 即可日常试用；D–E 通过后再用于真实残留清理。
