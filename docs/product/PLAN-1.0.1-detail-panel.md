# Remova 1.0.1 · 右侧详情面板升级 实施计划

> **状态**：已实施 · 真机验收通过（2026-09-17）  
> **目标版本**：`1.0.1`  
> **确认日期**：2026-09-16（方案审阅通过；实施从次日开始）  
> **验收日期**：2026-09-17（用户确认 §8 通过）  
> **本文用途**：1.0.1 详情面板升级的实施依据与验收记录。  
> **对照截图**：右侧详情抽屉——操作区 + 基本信息 + 关联项目（友好名/体积/可点）+ 深度卸载推荐卡

---

## 0. 已拍板决策（不可再开）

| # | 决策 | 结论 |
|---|---|---|
| D1 | 卸载模式（官方/深度/强制）是否收进 `⋯` | **是**。主按钮默认「深度卸载」 |
| D2 | 「关联项目」语义 | **v1 = 残留扫描分类**（非常驻库存）。接受 path 启发式归类 |
| D3 | 无 scan 时关联区 | 空态 +「深度分析」主 CTA |
| D4 | 体积 | **本版必须做**（1.0.1 进主线，不进 1.5） |
| D5 | drill-down | 跳主区残留列表，按 kind 过滤；不内嵌子面板 |
| 版本号 | `1.0.1` | `package.json` / `Cargo.toml` / `tauri.conf.json` 三处一致 |

---

## 1. 目标

1. 右侧详情改为决策卡布局：操作 → 基本信息 → 关联项目 → 深度卸载推荐
2. 关联项目：中文友好名 + 图标 + **数量**；file/dir 类显示 **体积**
3. 关联行可点击，主区残留列表按类过滤
4. 底部推荐卡展示残留总数，一键进入残留视图
5. 三模式卸载收进 `⋯` 菜单

### 非目标

- 不改 safety / `defaultSelectable` 谓词语义
- 不新增扫描面（浏览器插件、系统清理等）
- 不做「常驻库存」关联（无 scan 也显示程序文件/注册表等）

---

## 2. 现状锚点（实现时从这里读代码）

| 文件 | 作用 |
|---|---|
| `src/components/AppDetailPanel.tsx` | 右侧详情列（主改对象） |
| `src/lib/decision.ts` | `summarizeLeftovers` / `defaultSelectable` / `bucketItem` |
| `src/components/ScanLeftoversView.tsx` | 残留表（加 kind 过滤） |
| `src/App.tsx` | scan 状态、handler 接线（控制在 ≤800 行） |
| `src/i18n.ts` | 中英字典 |
| `src/types.ts` | `CleanupItem` / `ScanResult` |
| `src-tauri/src/scanner.rs` | `ItemKind` + 扫描产出（要加 size） |
| `src-tauri/src/lib.rs` | Tauri command 序列化（`CleanupItem` 透传） |
| `src-tauri/src/shared.rs` / 其它 push 点 | 所有构造 `CleanupItem` 的位置 |

后端真实 kind 仅四种：`file | dir | registry | path`（`ItemKind` Debug 小写）。

---

## 3. 信息架构（目标布局）

```text
┌─────────────────────────────┐
│ [icon] 名称                  │
│        版本 · 发布者         ×│
│ [ 卸载 ] [ 查看位置 ] [ ⋯ ]  │
├─────────────────────────────┤
│ 基本信息                     │
│   安装位置 / 占用 / 日期 / 版本│
│   （来源/健康度 → ⋯ 或次要行）│
├─────────────────────────────┤
│ 关联项目                     │
│   📁 程序文件      856 MB  › │
│   ⚙ 配置文件       21 MB  › │
│   🗄 注册表项        8 项  › │
│   🔗 快捷方式        3 项  › │
│   ▶ 启动项          2 项  › │
├─────────────────────────────┤
│ ✅ 深度卸载推荐              │
│   发现 47 项残留…   [查看详情]│
└─────────────────────────────┘
```

- 无 scan：关联区显示「深度分析」CTA；无推荐卡或推荐卡变为「扫描关联残留」
- 主按钮：`mode` 默认 `deep`，文案为深度卸载；点 `⋯` 可改模式并执行对应动作

---

## 4. 技术方案

### 4.1 原则

- 展示逻辑进独立模块；`AppDetailPanel` 只组装
- 安全谓词只用 `decision.ts::defaultSelectable`，禁止复制
- 体积只统计 **file/dir**；registry/path 永远不显示 size
- 体积为扫描期附带的 `size_kb`，不在面板打开时对路径再 stat（避免卡顿）

### 4.2 体积：后端（本版必做）

**问题**：`CleanupItem` 无 size 字段；截图需要「程序文件 856 MB」。

**方案**：

1. 扩展 `scanner::CleanupItem`（或共享结构）增加可选字段：
   ```rust
   pub size_kb: Option<u64>,  // file/dir 才填；registry/path 为 None
   ```
2. 序列化到前端：`size_kb?: number | null`
3. 产出规则：
   - `kind == File`：`fs::metadata(path).len() / 1024`
   - `kind == Dir`：目录递归求和（**必须有深度/条目上限**，例如 max depth 8 / max 5_000 entries / 单目录超时或失败则 `None`）
   - 失败 / 超限：`None`（前端显示 count 不显示 size）
4. 所有构造点都要带上 `size_kb`：
   - `scanner.rs` 各 push
   - `installmon.rs` 差分项
   - `orphans.rs`
   - `executor.rs` 若构造预览项
   - 测试夹具
5. 目录求和抽独立函数（如 `dir_size_kb_limited`），**必须有单测**（浅目录、超限截断、文件不可读）。

**性能注意**：

- 分析本身已在后台；目录求和可能拖长扫描 → 对超大目录截断并标记
- 不要为 registry/path 浪费 IO
- 若实测拖慢明显：对 install_location 外的 dir 只算一层或跳过（验收看真机）

**验收**：

- 分析 360/大体积软件后，程序文件行出现合理 MB/GB
- registry 行永远是「N 项」不是「0 MB」
- `cargo test --lib` 通过；扫描耗时相对 1.0.0 回归可接受（建议记录前后）

### 4.3 关联项目分类：前端

新文件 `src/lib/linkedItems.ts`：

```ts
export type LinkedBucketId =
  | "programFiles"
  | "configFiles"
  | "registry"
  | "shortcuts"
  | "startup"
  | "other";

export type LinkedBucket = {
  id: LinkedBucketId;
  labelKey: string;      // i18n key
  icon: string;          // 字符或内联标识
  count: number;
  sizeKb: number | null; // 仅 file/dir 聚合；无数据或 registry → null
  kinds: string[];       // drill-down 时用于过滤原始 kind/path
};

export function classifyItem(it: CleanupItem, app: InstalledApp): LinkedBucketId;
export function buildLinkedBuckets(items: CleanupItem[], app: InstalledApp): LinkedBucket[];
```

**分类规则（path 启发式，接受 D2）**：

| Bucket | 规则（按优先级） |
|---|---|
| `shortcuts` | `kind=file` 且 path 以 `.lnk` 结尾（不分大小写） |
| `startup` | registry path 含 `\CurrentVersion\Run` 或 `\Startup`；或 file 在 `Start Menu\Programs\Startup` |
| `registry` | `kind=registry`（且未归入 startup） |
| `configFiles` | `kind=file/dir` 且 path 含 `\AppData\`、`\ProgramData\`、或明显用户配置位 |
| `programFiles` | `kind=file/dir` 且（在 `install_location` 前缀下，或非 config 且在 Program Files 等） |
| `other` | 其余（含 `path` kind） |

- 排序固定：programFiles → configFiles → registry → shortcuts → startup → other
- 空桶不渲染
- `sizeKb`：桶内 file/dir 的 `size_kb` 求和；若桶内全是 registry/path 或全无 size → `null` 显示 count
- 单测表驱动：至少 8–10 条路径夹具（lnk、Run、AppData、install_location 下、未知）

### 4.4 Drill-down

1. `ScanLeftoversView` 增加 prop：`kindFilter?: LinkedBucketId | null`
2. `displayItems` 在非 orphan 时：`filterItemsByBucket(scan.items, app, kindFilter)`
3. 过滤函数与 `classifyItem` 同源（同一 `linkedItems.ts`）
4. UI：有过滤时顶栏显示「仅显示：程序文件 ×」可清除
5. 点击关联行：若已有匹配 scan → 直接 filter；否则先 `onAnalyze(app)`，结果到达后应用 filter（App 层 `pendingBucketFilter` 状态）

### 4.5 底部推荐卡

- 有 scan 且 `summary.total > 0`：
  - 标题：深度卸载推荐
  - 正文：发现 N 项残留，建议一并清理（N = total）
  - 副文案可选：可安全清理 X · 建议确认 Y（`summary.safe/suggest`）
  - CTA「查看详情」→ 清除 filter 或跳残留视图
- 无 scan：大按钮「深度分析」→ `onAnalyze`

### 4.6 `⋯` 菜单

折叠动作（复用现有 handler，禁止新开清理路径）：

- 官方卸载 → `onOfficialOnly`
- 深度卸载 → `onDeepUninstall`
- 强制清理 → `onForceClean`
- 深度分析 → `onAnalyze`

实现：轻量 popover（组件内 state），**不引第三方菜单库**。可选：复制路径。

主按钮始终执行当前选中 mode（默认 deep）；`⋯` 里点某模式可同时切换并执行，或仅切换——**实现取：菜单项直接执行对应动作，radio 移除**。

### 4.7 基本信息瘦身

保留：安装位置（含 ⧉）、占用空间、安装日期、版本号。  
来源、健康度：迁到 `⋯` 或次要 muted 行，避免首屏过长。

### 4.8 前端类型

```ts
// types.ts
export type CleanupItem = {
  // ...existing
  size_kb?: number | null;
};
```

---

## 5. i18n 新增键（中英都要）

| Key | 中文 | English |
|---|---|---|
| `linkedProgramFiles` | 程序文件 | Program files |
| `linkedConfigFiles` | 配置文件 | Config files |
| `linkedRegistry` | 注册表项 | Registry |
| `linkedShortcuts` | 快捷方式 | Shortcuts |
| `linkedStartup` | 启动项 | Startup |
| `linkedOther` | 其它 | Other |
| `deepUninstallRecommend` | 深度卸载推荐 | Deep uninstall tip |
| `foundNLeftovers` | 发现 {n} 项残留，建议一并清理 | {n} leftovers found — review and clean |
| `viewDetails` | 查看详情 | View details |
| `filterOnly` | 仅显示：{label} | Filtered: {label} |
| `showAllLeftovers` | 显示全部 | Show all |
| （菜单） | 官方卸载 / 深度卸载 / 强制清理 / 深度分析 | 已有 key 可复用则复用 |

现有可复用：`drawerOfficial` / `drawerDeepUninstall` / `rowForceClean` / `drawerAnalyze` / `linkedItems` / `basicInfo` / `openLocation`。

---

## 6. 任务清单（按顺序推进）

> 完成一项勾选并写日期。禁止跳过测试直接 UI。

| ID | 任务 | 验收 | 状态 |
|---|---|---|---|
| T1 | 后端：`size_kb` 字段 + 目录求和（限额）+ 全构造点填充 | `cargo test` 通过；手测扫描有体积 | ✅ 2026-09-17 |
| T2 | 前端 types + `lib/linkedItems.ts` 分类/聚合 + vitest | 表驱动测试全绿 | ✅ 2026-09-17 |
| T3 | i18n 键（中英） | 切换语言无缺 key | ✅ 2026-09-17 |
| T4 | `ScanLeftoversView` + `kindFilter` + 清除 | 手测过滤正确 | ✅ 2026-09-17（待真机） |
| T5 | `AppDetailPanel` 重构四区块 + `⋯` 菜单 | 视觉对齐目标布局 | ✅ 2026-09-17（待真机） |
| T6 | `App.tsx` 接线：pending filter、菜单动作、推荐卡 CTA | App.tsx ≤800 行 | ✅ 2026-09-17（718 行） |
| T7 | 全量验证：`cargo test` / `clippy` / `fmt` / `npm test` / `tsc+build` | 全部 PASS | ✅ 2026-09-17 |
| T8 | 真机验收清单（见 §8） | 用户确认 | ✅ 2026-09-17 |
| T9 | 版本号 1.0.1 三处对齐 + CHANGELOG + USER-GUIDE 详情面板说明 | 文档同步 | ✅ 2026-09-17 |

---

## 7. 风险与纪律

| 风险 | 缓解 |
|---|---|
| 目录求和拖慢扫描 / 扫爆盘 | depth + entry 上限；失败 `None`；真机对比耗时 |
| 分类启发式误归 | 兜底 `other`；**不改删除与勾选** |
| 多处构造 `CleanupItem` 漏字段 | T1 全局搜构造点；编译期靠 struct 字段或 Default |
| drill-down 与 selectedPaths 打架 | filter 只影响展示 |
| App.tsx 再次膨胀 | 面板逻辑进子组件/linkedItems；超 800 行先拆再交 |
| 测试碰真实系统 | 不改现有门禁；新测试只用临时目录夹具 |

**纪律**：

- 不 `as any` / `@ts-ignore`
- 不复制 `defaultSelectable`
- 新扫描/统计行为必须有测试
- 改完跑全量验证，禁止只声明「应该没问题」

---

## 8. 真机验收清单

- [x] 选中软件，右侧为四区块布局；无残留 radio 占首屏
- [x] 主按钮默认深度卸载；`⋯` 可官方/深度/强制/分析
- [x] 无分析：关联区分析 CTA；点击后开始扫描
- [x] 有分析：程序文件/配置文件等中文行；file 类有合理体积；注册表为「N 项」
- [x] 点「程序文件」等：主区残留表只显示该类；可「显示全部」
- [x] 底部推荐卡 N = 残留总数；CTA 进入残留视图
- [x] 默认勾选仍为 confirmed 且非高风险非 shared 非 user_data
- [x] 中英切换文案完整
- [x] 大体积软件分析时间可接受、UI 不假死
- [x] 安装监控 / 孤儿页不受回归影响（若扫到 size 字段）

> 用户确认：2026-09-17 真机验收通过。

---

## 9. 验证命令（Windows / PowerShell 7）

```powershell
# 后端
cd src-tauri
cargo fmt --check
cargo clippy --lib -- -D warnings
cargo test --lib

# 前端
cd ..
npm test
npm run build

# 版本一致性（实施 T9 后）
# package.json / src-tauri/Cargo.toml / src-tauri/tauri.conf.json 均为 1.0.1
```

---

## 10. 预估与节奏

| 时段 | 内容 |
|---|---|
| D1 上午 | T1 后端 size + 测试；T2 linkedItems + 测试 |
| D1 下午 | T3 i18n；T4 过滤；T5 面板 UI |
| D2 上午 | T6 接线；T7 全量验证 |
| D2 下午 | T8 真机验收 → 修边角；T9 版本与文档 |

合计约 **1.5–2 人日**（含体积后端）。若体积目录求和出现性能问题，优先保上限与测试，再微调策略。

---

## 11. Git 约束

- 未获用户明确确认前 **不 commit / 不 push**
- 提交前提供变更摘要：文件列表、关键实现、测试结果、潜在影响

---

## 12. 变更日志（本计划文件）

| 日期 | 变更 |
|---|---|
| 2026-09-16 | 初版：方案确认（D1–D5、体积入 1.0.1、版本 1.0.1）；待次日实施 |
| 2026-09-17 | T1–T7 落地；查看位置 os error 2 修复；真机验收通过；T9 文档补全 |

---

*实施时以本文为准；若与代码现实冲突，先改本文再改代码。*
