---
feature: remova-next-phase1
status: in-progress
updated: 2026-09-11
branch: main
commits: # filled at delivery
---

# Remova-next Phase 1 — 只读关联分析

## Report

## [S1] Problem

Phase 0 只有已安装软件列表。需要与 Python 版对齐的 **只读** 关联分析：安装目录 / AppData / 快捷方式 / 注册表产品键 / App Paths / Run 启动项，并展示证据与分数。

## [S2] Design

1. **Rust `scanner` 模块**
   - `slugify` / `_normalize_for_match` 对齐 Python
   - `extract_features`：name slugs、exe stems（确定性排序）
   - 扫描：InstallLocation 树；LOCALAPPDATA/APPDATA/PROGRAMDATA/Program Files 命名匹配；注册表卸载键 + Software 产品键 + App Paths（须落在安装目录）+ Run 值
   - 证据权重表与 `finalize`：score → confirmed/suspected/risk（与 Python 同阈值 90/30）
   - 输出 `CleanupItem { path, kind, score, confidence, risk, reason, evidence[] }`
2. **IPC** `analyze_associations(app) -> ScanResult`
3. **UI** 列表选中 →「深度分析」→ 预览表（路径/类型/分数/关联度/依据），**无删除按钮**
4. **验收**：`cargo test` 含 slug/证据/护栏；`npm run build` 通过

## [S3] Out of Scope

删除、备份、批量、服务删除执行、Python 自动 diff 脚本（下一轮）。

## Tasks

- [ ] T1: scanner 特征与证据评分 (covers: S2.1)
- [ ] T2: 文件系统/注册表扫描 (covers: S2.1)
- [ ] T3: command + React 预览 (covers: S2.2–S2.3)
- [ ] T4: cargo/npm 验收 (covers: S2.4)
