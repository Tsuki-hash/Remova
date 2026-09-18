# Remova 文档索引

> 更新：2026-09-18 · 当前版本 **1.1.0**（与 `package.json` / `Cargo.toml` / `tauri.conf.json` 一致）

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

## 评审记录（历史，本地/内部）

> `docs/reviews/` 默认不进公开 git（见 `.gitignore`）。完整问题清单与改进进度见  
> `reviews/2026-09-18-全面系统性评审.md`。

| 文档 | 日期 |
|---|---|
| reviews/2026-09-14-项目评审报告.md | 2026-09-14 |
| reviews/2026-09-14-项目评审报告-第二轮.md | 2026-09-14 |
| reviews/20260915-项目全面评审.md | 2026-09-15 |
| reviews/2026-09-16-项目评审报告-第三轮.md | 2026-09-16 |
| reviews/2026-09-16-评审落地复核-第四轮.md | 2026-09-16 |
| reviews/2026-09-17-全面系统性评审.md | 2026-09-17 |
| reviews/2026-09-18-全面系统性评审.md | 2026-09-18 |

## 归档（只读，内部）

- `archive/MIGRATION.md` — Python 旧版迁移说明  
- `archive/compose/spec/` — 迁移期 compose 规格（已交付）

## 目录约定

```text
docs/
  README.md          本索引
  USER-GUIDE.md      面向用户（进 git）
  ARCHITECTURE.md    面向开发者（进 git）
  product/           产品规划与路线（进 git）
  reviews/           带日期的评审快照（默认私有）
  compose/           活跃规格（默认私有）
  archive/           已完结/不再维护（默认私有）
```

新建文档时：活文档放根目录或 `product/`；评审稿写入 `reviews/` 并带日期；废弃内容进 `archive/`。  
**版本叙事**：文档「当前版本」必须等于 `package.json` 的 `version`。
