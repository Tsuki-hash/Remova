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

**核心路径（Deep Uninstall）**：选择软件 → 查看详情 → 深度卸载 → 官方卸载程序 → 自动扫描残留 → 证据分类 → 用户确认 → 可选备份 → 清理 → 报告/可恢复。

> 普通卸载：把程序删掉。  
> Remova：找到程序、卸载程序、追踪关联、识别残留、解释风险、最后清干净。

| 能力 | 说明 |
|---|---|
| **决策列表** | 点行打开右侧详情：体积、关联分桶、健康度；普通/深度/强制三模式 |
| **深度卸载闭环** | 确认框写清流程；阶段条（识别→官方卸载→扫描→分析→报告）；清理后报告与复核 |
| **证据扫描** | 安装目录、快捷方式、TEMP、注册表、Run、服务、计划任务、**PATH、CLSID/Shell、驱动、跨盘浅根、WebView2 缓存、主目录点目录、MuiCache** |
| **说人话** | 每条残留标明「为什么删 / 为什么保留」；安全 · 建议确认 · 将保留 三档 |
| **判断依据** | 孤儿残留可展开：无卸载记录 / 含 exe / 配置文件 / 安装根目录 / 最近修改等证据 |
| **深度卸载补充** | 损坏的 MSI 安装记录、孤儿防火墙规则、失败项锁占用诊断（确认后可结束安装目录内的进程并重新扫描/重试）；报告显示实删/保留/待重启体积，可重新扫描或做只读深度注册表核查 |
| **用户数据红线** | Documents / Downloads / 同步冲突目录默认不勾，避免误删你的文件 |
| **删除保险箱** | 清理前**可选**备份（确认框默认不勾）；会话保留 7 天；报告与历史可打开已有备份 |
| **可验证** | 清理后复核 checklist（路径 / 注册表 / PATH 是否真的清掉） |
| **孤儿残留** | 一级页面专列：无安装记录的残留，按来源分组 |
| **系统项语义** | 启动项=已启用/已禁用；服务拆分运行状态与启动类型（停止 ≠ 禁用）；计划任务展示上次/下次运行 |
| **智能能力（可选）** | 智能筛选 / 模型设置 / 残留解释——只建议，不替代你确认 |

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
4. **清理报告** 已删/失败/跳过 + 实删/保留体积 + 完成度 + 保险箱说明 + 复核结果；可重新扫描该应用  

### 清理与备份
- 仅预览 dry-run；确认时可勾选「清理前创建安全备份」（**默认不勾**）再删  
- 清理前默认尝试创建系统还原点，可在「更多」中关闭  
- 共享运行库（VC++ / .NET 等）默认不勾  
- 备份根目录（仅勾选备份时生成）：`%PROGRAMDATA%\Remova\Backup\`

### 更多工具
- 孤儿残留页、启动项 / 服务 / 计划任务管理；损坏的安装记录与孤儿网络规则面板  
- 安装包与更新缓存、专项缓存清理、磁盘雷达、闲置软件雷达；扫描结果会标注「另有 N 项未显示」与读不到的位置  
- **C 盘空间体检**：概览（卷容量、目录链接与死链）、清理与迁移（处置清单，可清理项置顶并可勾选清理，迁移候选交给目录迁移）、组件存储（WinSxS DISM 只读分析）
- **可再生缓存清理**：仅限固定布局（更新下载缓存、临时目录、崩溃转储等），确认时明示不创建备份；只删内容、目录保留，被占用文件如实跳过  
- **目录迁移（junction）**：把目录整体迁往其他盘——计划 → 复制（逐文件校验）→ 切换（失败自动回滚）→ 收尾（确认后删备份）；随时可一键回滚  
- **全局 CLI 包**：npm / pnpm / yarn 全局包只读清单；npm 包可在确认后删除（先整包导出到安全备份）  
- **系统痕迹与便携应用**：ConsentStore 孤儿授权与便携应用候选的只读报告（不过安全评估的类别一律不落地）  
- 历史时间线、CSV 导出、安装监控  
- 深色 / 浅色 / 跟随系统主题，中文 / English

---

## 下载安装

前往 **[Releases](https://github.com/Tsuki-hash/Remova/releases)** 下载最新版本：

> 应用内更新未启用（未配置 updater 公钥）；升级一律以 Release 页面手动下载为准。若 Release 未提供 Authenticode 签名，请用同页 `SHA256SUMS.txt` 校验安装包。配置更新公钥后，Release 会额外提供 `latest.json` 与 `.sig`（应用内更新器清单及其签名文件）；未配置时不会生成，MSI 安装也不接入自动更新，以手动下载为准。
>
> 发布模式：未配置 updater 密钥时，Release 走「未签名 / 手动更新」模式，只发布 NSIS / MSI / 便携包与 `SHA256SUMS.txt`，不生成 `latest.json` 与 `.sig`；三项密钥齐全时才生成并校验更新签名资产。

| 安装包 | 说明 |
|---|---|
| `Remova_*_x64-setup.exe` | NSIS 安装程序（推荐） |
| `Remova_*_x64_zh-CN.msi` | MSI 安装程序（per-machine，仅中文向导；非管理员需 UAC 提权） |
| `Remova_*_x64-portable.zip` | 便携版 zip，解压即用 |

系统要求：Windows 10 / 11（x64），以及 Microsoft Edge WebView2 Runtime（Windows 11 与多数 Windows 10 已内置；便携版不附带，缺失时需先安装）。NSIS 安装器在缺少 WebView2 时会联网下载；离线/内网环境请先安装 WebView2，或改用已预装的 MSI/便携包。清理系统级应用、Machine PATH/服务/防火墙以及还原到受保护目录时，需要以管理员身份运行；普通权限下这些条目会逐条失败并报告原因。

> **便携试用**：可从本机 `src-tauri/target/release/remova.exe` 复制运行；注册表扫描无需安装。用户备份默认在 `%PROGRAMDATA%\Remova\Backup\`。正式分发以 NSIS / MSI 为准。

---

## 快速开始（开发者）

### 环境

| 工具 | 版本 |
|---|---|
| Rust | ≥ 1.77.2 |
| Node.js | 22 |
| Windows | 10 / 11 |

### 命令

```powershell
# 前端
npm ci
npm run build
npm run coverage

# 后端
cd src-tauri
cargo test --workspace
# 热点纯函数微基准（可选，仅本地）
cargo bench

# 开发运行
npx tauri dev

# 打包
npx tauri build
```

贡献规范见 [CONTRIBUTING.md](./CONTRIBUTING.md)。

---

## 安全说明

- 真实删除必须经过 UI 确认，并通过 `safety` 门禁检查。
- 分析和清理只使用最近一次服务端扫描的应用身份与路径；HKCU 与 HKLM 的同名记录不会合并，提权状态会拒绝 HKCU 应用的分析、卸载与清理。
- 优先使用**仅预览**确认列表，再执行清理。
- 备份是尽力而为，**不能替代系统备份**；备份目录使用私有 ACL（SYSTEM / Administrators / 当前用户）。
- 1641 待重启期间所有清理与还原入口保持阻断；状态文件损坏时按“待重启”处理（fail-closed）。
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
├── public/              # 构建时原样复制的静态资源（logo.png）
├── src-tauri/           # Rust 后端与打包配置
├── scripts/             # 构建 / 辅助脚本
├── .github/
│   ├── workflows/       # CI / Release
│   ├── ISSUE_TEMPLATE/  # Issue 表单
│   ├── signpath/        # SignPath 签名工件配置
│   └── RELEASE_SIGNING.md
├── index.html           # Vite 入口 HTML
├── package.json         # 前端依赖与脚本（版本号唯一源）
├── package-lock.json    # 锁定依赖树
├── tsconfig.json        # TypeScript 配置
├── vite.config.ts       # Vite / Vitest 配置
├── eslint.config.mjs    # ESLint 配置
├── CHANGELOG.md
├── CONTRIBUTING.md
├── LICENSE
├── README.md            # 本文件（中文）
└── README.en.md         # English
```

> 版本号以 `package.json` 的 `version` 为准；`Cargo.toml` / `Cargo.lock` / `tauri.conf.json` / `package-lock.json` 由 `npm run check-versions` 对齐。

---

## 贡献

欢迎 Issue 与 Pull Request：

1. Fork 并创建功能分支
2. 运行 `cargo test --workspace` 与 `npm run coverage`
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
