<div align="center">

<img src="src-tauri/icons/icon.png" alt="Remova" width="96" />

# Remova

**Windows 深度卸载 · 删得干净，也知道删了什么**

找到残留 → 解释原因 → 安全删除 → 随时恢复

[English](./README.en.md) · 简体中文

[![Release](https://img.shields.io/github/v/release/Tsuki-hash/Remova?style=flat-square&label=release)](https://github.com/Tsuki-hash/Remova/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Tsuki-hash/Remova/total?style=flat-square)](https://github.com/Tsuki-hash/Remova/releases)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](./LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6?style=flat-square)](https://github.com/Tsuki-hash/Remova/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/Tsuki-hash/Remova/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/Tsuki-hash/Remova/actions)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)
[![React](https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=black)](https://react.dev)
[![Rust](https://img.shields.io/badge/Rust-1.77%2B-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)

</div>

---

## 为什么选择 Remova

普通卸载只删主程序；注册表、服务、PATH、CLSID、跨盘缓存常被留下，传统清理器又容易误删用户文件。

Remova 的原则是 **Find it. Explain it. Remove it. Restore it.**（找到 · 解释 · 删除 · 恢复）：

| 能力 | 说明 |
|---|---|
| **决策列表** | 点开详情抽屉再卸载：体积、关联、健康度、普通/深度/强制三模式 |
| **证据扫描** | 安装目录、快捷方式、TEMP、注册表、Run、服务、计划任务、**PATH、CLSID/Shell、驱动、跨盘浅根、WebView2 缓存** |
| **说人话** | 每条残留标明「为什么删 / 为什么保留」；安全 · 建议确认 · 将保留 三档 |
| **用户数据红线** | Documents / Downloads / 同步冲突目录默认不勾，避免误删你的文件 |
| **删除保险箱** | 清理前自动备份；会话保留 7 天；报告与历史可一键打开备份目录 |
| **可验证** | 清理后复核 checklist（路径 / 注册表 / PATH 是否真的清掉） |
| **孤儿残留** | 一级入口：扫描无安装记录的残留，按疑似来源分组后再清理 |
| AI 辅助（可选） | 残留解释、风险摘要、自然语言计划——只建议，不替代你确认 |

> 产品气质：克制、专业、可后悔——不是「电脑管家」。

---

## 功能一览

### 已安装软件列表
- 来源：HKLM64 / HKLM32 / HKCU / Microsoft Store（MSIX），同产品自动去重
- 决策 chips：大体积 / 最近安装 / 推荐清理 / 无卸载命令 / 商店或桌面
- 搜索、排序、虚拟化大列表；拖放 exe / 安装目录可分析

### 深度卸载流程
1. **识别** 安装类型与关联  
2. **官方卸载** 优先调用软件自带卸载器  
3. **扫残留** 实时概况（可安全清理 / 建议确认 / 将保留）  
4. **清理报告** 已删/失败/跳过 + 完成度 + 保险箱说明 + 复核结果  

### 清理与备份
- 仅预览 dry-run；确认后自动备份再删  
- 共享运行库（VC++ / .NET 等）默认不勾  
- 备份根目录：`%PROGRAMDATA%\Remova\Backup\`

### 更多工具
- 孤儿残留页、启动项 / 服务 / 计划任务管理  
- 历史时间线、CSV 导出、安装监控  
- 深色 / 浅色主题，中文 / English

---

## 下载安装

前往 **[Releases](https://github.com/Tsuki-hash/Remova/releases)** 下载最新版本：

| 安装包 | 说明 |
|---|---|
| `Remova_*_x64-setup.exe` | NSIS 安装程序（推荐） |
| `Remova_*_x64_en-US.msi` | MSI 安装程序 |
| `Remova.exe` | 便携试用（见下方说明） |

系统要求：Windows 10 / 11（x64）。

> **便携试用**：可从本机 `src-tauri/target/release/remova.exe` 复制运行；注册表扫描无需安装。用户备份默认在 `%PROGRAMDATA%\Remova\Backup\`。正式分发以 NSIS / MSI 为准。

---

## 快速开始（开发者）

### 环境

| 工具 | 版本 |
|---|---|
| Rust | ≥ 1.77 |
| Node.js | 22 |
| Windows | 10 / 11 |

### 命令

```powershell
# 前端
npm ci
npm run build
npm test

# 后端
cd src-tauri
cargo test --lib

# 开发运行
npx tauri dev

# 打包
npx tauri build
```

贡献规范见 [CONTRIBUTING.md](./CONTRIBUTING.md)。

---

## 安全说明

- 真实删除必须经过 UI 确认，并通过 `safety` 门禁检查。
- 优先使用**仅预览**确认列表，再执行清理。
- 备份是尽力而为，**不能替代系统备份**。
- 服务 / 计划任务默认视为高风险，且默认不勾选。

---

## 技术栈

```text
UI     React 19 + Vite + TypeScript
壳层   Tauri 2
后端   Rust（注册表 / 文件系统 / 服务 / 任务）
测试   Vitest + cargo test
打包   NSIS / MSI
```

---

## 目录结构（公开）

```text
Remova/
├── src/                 # React 前端
├── src-tauri/           # Rust 后端与打包配置
├── scripts/             # 构建 / 辅助脚本
├── .github/workflows/   # CI / Release
├── CHANGELOG.md
├── CONTRIBUTING.md
├── README.md            # 本文件（中文）
└── README.en.md         # English
```

---

## 贡献

欢迎 Issue 与 Pull Request：

1. Fork 并创建功能分支
2. 运行 `cargo test --lib` 与 `npm test`
3. 使用 Conventional Commits（`feat:` / `fix:` / `docs:` …）
4. 用户可见变更请更新 `CHANGELOG.md`

详见 [CONTRIBUTING.md](./CONTRIBUTING.md)。

---

## 许可证

[MIT License](./LICENSE)

---

## 免责声明

请在审阅清理列表后自行判断风险再执行。备份为尽力而为，不构成完整系统备份或数据恢复保证。

---

<div align="center">

如果 Remova 对你有帮助，欢迎点个 ⭐ Star 支持

[![Star](https://img.shields.io/github/stars/Tsuki-hash/Remova?style=social)](https://github.com/Tsuki-hash/Remova)

</div>
