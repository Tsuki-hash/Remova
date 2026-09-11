# Remova 迁移说明

**状态：完整迁移已完成（功能级）** — 2026-09-11

| 项 | 内容 |
|---|---|
| 新版 | Remova-next（Tauri 2 + React 19 + Rust） |
| 仓库 | https://github.com/Tsuki-hash/Remova-next |
| 版本 | v1.0.0（验收通过）+ 完整扫描/UI 补齐（本批） |
| 旧版 | https://github.com/Tsuki-hash/Remova（Python，保留对照） |

## 已完整覆盖

- 已安装软件枚举（与 Python **100%** 列表一致）
- 关联分析：安装目录、AppData/ProgramData、**快捷方式**、**TEMP**、卸载键、**Software\\Product**、App Paths、Run、服务、计划任务
- dry-run / 备份 / 删除 / 还原（最近或指定会话）/ 历史 / CSV
- 官方卸载器开关、批量清理、主题、中英、磁盘、引导、版本检查
- 备份失败中止、safety 门禁、MoveFileEx、还原点、提权
- 真机验收清单 A–G 通过

## 与旧版关系

- 新版为**默认推荐**实现
- 旧版 Python 可继续作只读对照或紧急回滚
- 后续功能请在 Remova-next 上迭代

## 公开仓库前建议（可选）

1. 代码签名证书（去 SmartScreen）
2. LICENSE / 贡献指南已在旧版；可复制到 Remova-next
3. 介绍截图与 README 完善
4. 确认隐私策略（仅本地数据：备份/历史/指标）
