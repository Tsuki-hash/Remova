# Windows 候选版本性能阈值

在测量前确定本文件的阈值。用最终候选可执行文件执行启动测量。

此流程用于选做的性能专项。发布签批使用同一候选的本机双权限 WebView2 启动记录。

## 1. 适用范围

| 项目 | 值 |
|---|---|
| 被测版本 | 固定提交和可执行文件 SHA-256 的候选版本 |
| 测试环境 | 同一台 Windows x64 机器、WebView2 版本和权限模式 |
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

### 指标口径

- 目录大小行走夹具包含 4,040 项，低于应用每次 5,000 项的扫描上限。
- UI Automation 观察到可用、可见的软件导航按钮时，记为可交互。应用列表加载由安装包验收单独核对。
- 工作集在导航就绪后采集。六个指标按绝对上限评估。

## 3. 测量方法

在候选提交的干净检出上测量。每次重新启动 Windows 后，在首次打开 Remova 前运行一次冷启动命令。使用同一输出目录累计至少三次不同 Windows 启动会话的样本。每次记录导航就绪耗时和随后两秒的主进程工作集。`-Mode warm` 只生成诊断记录。

```powershell
$candidateCommit = (git rev-parse HEAD).Trim()
$exe = "D:\artifacts\Remova.exe"
$exeSha = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
$out = "D:\acceptance\performance"
pwsh -NoProfile -File scripts/acceptance/measure-perf.ps1 -Mode cold -ConfirmFirstLaunchSinceBoot `
  -Exe $exe -Out $out -CandidateCommit $candidateCommit
pwsh -NoProfile -File scripts/acceptance/measure-components.ps1 -Startup (Join-Path $out 'startup-cold.json') `
  -CandidateCommit $candidateCommit -CandidateExeSha256 $exeSha -Run
```

要重新判定已有组件记录，传入 `-Components <components.json>`。启动样本、组件记录、预期候选和阈值表摘要须一致。六项完整且达标退出 0，超限退出 1，缺少数据、阈值、就绪证据或身份不符退出 2。归档两份 JSON 和实际运行环境。

## 4. 判定规则

- 任一指标超过绝对上限，性能检查失败。
- 未测或数据不完整 → 记为「证据不足」，不得判为通过。
- 阈值调整只能由维护者在测量前修改本文件；测量之后修改上限需要书面说明理由并重新测量。
