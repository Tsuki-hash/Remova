# Remova 文档索引

> 更新：2026-09-16 · 当前版本 **1.3.1**

## 必读（活文档）

| 文档 | 用途 |
|---|---|
| [USER-GUIDE.md](./USER-GUIDE.md) | 用户手册：安装、主流程、高级工具 |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | 架构：模块地图、命令、线程模型、安全 |
| [product/AI-FEATURES.md](./product/AI-FEATURES.md) | AI 路线图（M1–M4 已落地 1.3.0+） |
| [product/PRODUCT-GAPS.md](./product/PRODUCT-GAPS.md) | 产品差距与 backlog |

## 产品 / 对照

| 文档 | 用途 |
|---|---|
| [product/PARITY.md](./product/PARITY.md) | 与旧版 / 竞品能力对照 |
| [product/ACCEPTANCE.md](./product/ACCEPTANCE.md) | 验收清单 |

## 评审记录（历史）

| 文档 | 日期 |
|---|---|
| [reviews/2026-09-14-项目评审报告.md](./reviews/2026-09-14-项目评审报告.md) | 2026-09-14 |
| [reviews/2026-09-14-项目评审报告-第二轮.md](./reviews/2026-09-14-项目评审报告-第二轮.md) | 2026-09-14 |
| [reviews/20260915-项目全面评审.md](./reviews/20260915-项目全面评审.md) | 2026-09-15 |
| [reviews/Remova_UI_交互与创新建议_9.15.md](./reviews/Remova_UI_交互与创新建议_9.15.md) | 2026-09-15 |

## 归档（只读）

- [archive/MIGRATION.md](./archive/MIGRATION.md) — Python 旧版迁移说明  
- [archive/release-notes-v1.1.0.md](./archive/release-notes-v1.1.0.md) — 历史发版说明  
- [archive/compose/spec/](./archive/compose/spec/) — 迁移期 compose 规格（已交付，勿再改）

## 目录约定

```text
docs/
  README.md          本索引
  USER-GUIDE.md      面向用户
  ARCHITECTURE.md    面向开发者
  product/           产品规划与路线（可改）
  reviews/           带日期的评审快照（只追加）
  archive/           已完结/不再维护
```

新建文档时：活文档放根目录或 `product/`；评审稿写入 `reviews/` 并带日期；废弃内容进 `archive/`。
