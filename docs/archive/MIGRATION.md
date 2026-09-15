# Remova 迁移说明

**状态：完整迁移已完成；主仓已替换为 Tauri 版并公开** — 2026-09-11

| 项 | 内容 |
|---|---|
| 当前主仓 | https://github.com/Tsuki-hash/Remova（**public**，Tauri 2 + React 19 + Rust） |
| 版本 | v1.0.0+（验收通过 + 完整扫描/UI） |
| 旧 Python 仓 | https://github.com/Tsuki-hash/Remova-python-legacy（对照；删除需 `delete_repo` 权限） |
| 本地 | 新版 `Remova-next\`；旧版 `Trash\` → legacy 远程 |

## 已完整覆盖

- 已安装软件枚举（与 Python **100%** 列表一致）
- 关联分析：安装目录、AppData/ProgramData、快捷方式、TEMP、卸载键、Software\Product、App Paths、Run、服务、计划任务
- dry-run / 备份 / 删除 / 还原 / 历史 / CSV / 批量 / 主题 / 中英 / 磁盘 / 提权 / MoveFileEx / 还原点
- 真机验收 A–G 通过

## 关系

- **Tsuki-hash/Remova** 为唯一主仓
- Python 版在 **Remova-python-legacy**
- 后续只在 Tauri 版迭代

## 彻底删除旧仓（可选）

```powershell
gh auth refresh -h github.com -s delete_repo
gh repo delete Tsuki-hash/Remova-python-legacy --yes
```
