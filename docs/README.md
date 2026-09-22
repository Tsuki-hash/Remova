# Remova 文档索引

> 更新：2026-09-21 · 当前版本 **1.1.1**（与 `package.json` / `Cargo.toml` / `tauri.conf.json` 一致）

## 必读（活文档）

| 文档 | 用途 |
|---|---|
| [USER-GUIDE.md](./USER-GUIDE.md) | 用户手册：安装、主流程、高级工具 |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | 架构：模块地图、命令、线程模型、安全 |
| [product/AI-FEATURES.md](./product/AI-FEATURES.md) | AI 功能说明（基线 1.1.0 AI 决策层） |
| [product/PRODUCT-GAPS.md](./product/PRODUCT-GAPS.md) | 产品差距与 backlog |

## 产品 / 对照

| 文档 | 用途 |
|---|---|
| [product/PARITY.md](./product/PARITY.md) | 与旧版 / 竞品能力对照 |
| [product/ACCEPTANCE.md](./product/ACCEPTANCE.md) | 验收清单 |
| [product/PLAN-AI-decision-layer.md](./product/PLAN-AI-decision-layer.md) | AI 决策层计划（已落地） |
| [product/PLAN-1.0.1-detail-panel.md](./product/PLAN-1.0.1-detail-panel.md) | 详情面板计划（功能并入 1.1.0） |

## 评审记录

内部评审与改进进度位于本地 **`docs/reviews/`**（`.gitignore`，默认不进公开仓）。  
产品版本与变更以本页顶部版本行与 `CHANGELOG.md` 为准。

## 归档（只读，内部）

- `docs/archive/` — 迁移与已完结规格（不公开）

## 目录约定

```text
docs/
  README.md          本索引
  USER-GUIDE.md      面向用户（进 git）
  ARCHITECTURE.md    面向开发者（进 git）
  product/           产品规划与路线（进 git）
  reviews/           评审快照（默认私有）
  compose/           规格（默认私有）
  archive/           归档（默认私有）
```

**版本叙事**：文档「当前版本」必须等于 `package.json` 的 `version`。
