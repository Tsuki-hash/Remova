# Remova-next 真机验收清单

**状态：已通过（用户确认，2026-09-11）**  
基线：Release `v0.1.0` / `npm run tauri dev`；列表对比 coverage 100%。

## A. 只读（无风险）

- [x] 启动后列表显示约 200+ 软件；与 Python `python main.py --list` 数量接近
- [x] 搜索、列排序、深浅色、中英切换正常
- [x] 状态栏显示管理员/磁盘
- [x] `scripts/compare_lists.py` coverage ≥ 85%

## B. 分析（无删除）

- [x] 选中带 InstallLocation 的软件 → 深度分析
- [x] 应出现：安装目录（★★★）+ 卸载注册表键
- [x] 无安装路径的软件不应出现 App Paths 项

## C. dry-run

- [x] 演练清理：计划删除数 = 分析确定项；不改系统

## D. 备份清理（高风险——仅测试软件）

- [x] 创建可丢弃的测试目录 + Run 值，或使用便携软件
- [x] 备份并清理（勿勾官方卸载器）
- [x] 确认 `%PROGRAMDATA%\Remova\Backup\` 有会话
- [x] 确认测试目录/注册表消失或 delayed
- [x] 历史有记录；导出 CSV

## E. 还原

- [x] 还原最近备份后测试文件重新出现

## F. 批量

- [x] 多选两个测试项 → 批量清理，进度 notice 更新

## G. 回归

- [x] 不误删：服务 Winmgmt 等关键项不在结果或不可删
- [x] Microsoft 计划任务不出现

**结论（2026-09-11）**：A–G 通过，Remova-next 可作为 Python Remova 的功能级替换（v1.0.0）。
