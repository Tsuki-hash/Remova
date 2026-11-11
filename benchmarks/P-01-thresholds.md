# P-01 性能与容量阈值

**状态：已确认（维护者于 2026-11-10 确认本节六项上限；同日的候选版本实测见第 4 节）。**
第 4 节保留历史样本。阈值须在测量前定稿，当前候选仍需按以下方法采证。

## 1. 适用范围

| 项目 | 值 |
|---|---|
| 被测版本 | 1.4.1 工作区（`main`，候选构建） |
| 参照版本 | 无（本机只保留了候选版本；相对 +20% 一列改为「绝对上限」判定，参照对照留待有第二份构建时补） |
| 测试机器 | 联想 82WK / Windows 11 家庭版 26200 / x64 / SSD / 非提权会话 |
| 数据规模 | 启动与工作集为真实应用；扫描/分析/备份为临时目录合成夹具 |

## 2. 阈值（默认值）

| 指标 | key | 绝对上限 |
|---|---|---|
| 冷启动到可交互 | startup_worst_ms | 3000 |
| 峰值工作集（空闲） | idle_working_set_mb | 250 |
| 目录大小行走 4,040 项 | walk_worst_ms | 4000 |
| 单应用关联分析 200 文件 | analyze_worst_ms | 3000 |
| 取消传播（cancel → worker 观测） | cancel_worst_ms | 500 |
| 备份 1 GiB 数据 | backup_ms | 60000 |

**取值依据**：冷启动 3 秒是桌面工具的可接受上限；4,040 项目录行走 4 秒对应约 1,000 项/秒，远低于实测吞吐，只用于拦住数量级退化；单应用关联分析 3 秒覆盖 200 个可执行文件的安装目录并留出 3 倍余量；取消 500 ms 对应「点击后立刻有反馈」的感知阈值；工作集 250 MB 按 150 个应用的列表加扫描峰值估算；1 GiB 备份 60 秒对应约 17 MB/s，是机械盘也能达到的下限。

### 指标口径说明（相对评审原文的调整）

| 原提议 | 现口径 | 原因 |
|---|---|---|
| 扫描 1 万 / 10 万项目录 | 目录大小行走 4,040 项 | 应用真实的目录大小行走有 5,000 项/次的硬上限（`dirsize::MAX_WALK_ENTRIES`），10 万项无法经由产品代码路径产生，测出来的只会是夹具生成器的速度 |
| 应用列表首屏渲染 1500 ms | 并入「冷启动到可交互」 | 首个有效样本以 UI Automation 观察到可用、可见的软件导航按钮为准。应用列表数据完全加载另由关键路径验收核对。 |
| 峰值工作集（扫描中） | 未纳入 | 需要在扫描进行中采样进程内存；当前无脚本化扫描入口，不伪造 |
| 相对参照版本 +20% | 绝对上限 | 本机只有候选版本；构建 1.4.0 后可用 `measure-components.ps1` 的同口径数据补对照 |

## 3. 测量方法

关闭运行中的 Remova。在候选 SHA 对应的干净检出上使用同一候选主程序和同一台机器测量。启动脚本记录至少三次独立进程启动到 WebView2 软件导航按钮就绪的耗时，并在每次就绪后采样两秒主进程工作集。正式冷启动样本应从干净会话开始。

```powershell
$candidateCommit = (git rev-parse HEAD).Trim()
$exe = "D:\artifacts\Remova.exe"
$exeSha = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
$out = "D:\acceptance\performance"
pwsh -NoProfile -File scripts/acceptance/measure-perf.ps1 -Exe $exe -Out $out -CandidateCommit $candidateCommit
pwsh -NoProfile -File scripts/acceptance/measure-components.ps1 -Startup (Join-Path $out 'P-01-startup.json') `
  -CandidateCommit $candidateCommit -CandidateExeSha256 $exeSha -Run
```

要重新判定已有组件记录，传入 `-Components <components.json>`。启动样本、组件记录、预期候选和阈值表摘要须一致。六项完整且达标退出 0，超限退出 1，缺少数据、阈值、就绪证据或身份不符退出 2。归档两份 JSON 和实际运行环境。

## 4. 结果记录

**候选版本实测（2026-11-10）**

| 指标 | 实测（中位 / 最差） | 上限 | 结论 |
|---|---|---|---|
| 冷启动到可交互 | 首次 2286 ms；热启动中位 107 ms / 最差 110 ms | 3000 ms | PASS |
| 峰值工作集（空闲） | 约 26 MB | 250 MB | PASS |
| 目录大小行走 4,040 项 | 4.2 ms / 5.0 ms | 4000 ms | PASS |
| 单应用关联分析 200 文件 | 862.8 ms / 1118.1 ms | 3000 ms | PASS |
| 取消传播 | 1.09 ms / 1.30 ms | 500 ms | PASS |
| 备份 1 GiB 数据 | 4512.4 ms | 60000 ms | PASS |

原始数据：`benchmarks/runs/P-01-startup.json`、`benchmarks/runs-warm/P-01-startup.json`，以及组件测量输出的 `components.json`（路径见对应 `perf-*` 目录）。

## 5. 判定规则

- 任一指标超过绝对上限 → P-01 不通过。
- 未测或数据不完整 → 记为「证据不足」，不得判为通过。
- 阈值调整只能由维护者在测量前修改本文件；测量之后修改上限需要书面说明理由并重新测量。
