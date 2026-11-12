# 一次性 Windows VM 验收套件

本套件用于完成终审报告中必须由隔离真机提供的证据：

- **凭据与授权**：凭据绑定、重定向、孤儿授权分类与权限矩阵
- **安装矩阵**：安装 / 升级 / 降级 / 回滚与关键路径
- **性能基线**：性能与容量基线（阈值由维护者先定，见第 5 节）

**只能在一次性 Windows VM 快照上运行。** 脚本会在启动时拒绝在非一次性环境执行，但请把这条当作第二道保险，而不是第一道。

---

## 1. 前置条件

| 项目 | 要求 |
|---|---|
| 环境 | Windows 10 或 11 x64，**一次性 VM 快照**，可随时回滚 |
| 权限 | 需要一次管理员令牌与一次普通用户令牌（两组结果都要） |
| 产物 | 待验收的安装包：NSIS、MSI、portable zip，来自同一 commit |
| 校验 | 每个产物的 SHA-256，与 `SHA256SUMS.txt` 一致 |
| 网络 | 重定向检查需要访问本机回环；其余场景建议断网运行，避免真实外部请求 |

**不要在以下环境运行**：日常开发机、生产桌面、共享 CI runner、任何无法整体回滚的机器。

---

## 2. 运行方式

在用于审查发行附件的 Windows 机器上，切换到候选提交，用可信的 7-Zip 和 WiX 3 `dark.exe` 生成可执行文件参考摘要。脚本只解包，并逐字节核对三种包内的 `LICENSE` 和 `THIRD-PARTY-NOTICES.md`。把生成的 JSON 随同一批附件交给各验收环境和签批人。

```powershell
./scripts/acceptance/get-package-exe-reference.ps1 -Artifacts 'D:/artifacts' -CandidateVersion 1.4.1 `
    -SevenZip 'C:/Program Files/7-Zip/7z.exe' -WixDark 'C:/Tools/WiX/bin/dark.exe' |
    Set-Content -Encoding utf8 'D:/acceptance/package-exe-reference.json'
```

```powershell
# 1. 确认是隔离 VM 并接受风险（脚本会二次确认）
$env:REMOVA_ACCEPTANCE_ISOLATED_VM = "1"
$env:REMOVA_ACCEPTANCE_ARTIFACTS  = "D:\artifacts"   # 存放三个安装包
$env:REMOVA_ACCEPTANCE_OUT        = "D:\acceptance"  # 结果输出目录

# 2. 用候选构建记录里的完整提交 SHA 替换占位符
$candidateCommit = "<候选提交的 40 位 SHA>"
$packageExeReference = 'D:/acceptance/package-exe-reference.json'
pwsh -NoProfile -File scripts/acceptance/run-acceptance.ps1 -Suite client -CandidateCommit $candidateCommit `
    -CandidateVersion 1.4.1 -PackageExeReference $packageExeReference

# 3. 另开管理员 PowerShell，使用相同的包和候选身份运行
pwsh -NoProfile -File scripts/acceptance/run-acceptance.ps1 -Suite elevated -CandidateCommit $candidateCommit `
    -CandidateVersion 1.4.1 -PackageExeReference $packageExeReference
```

在 Windows 10 和 11 上各留存两份摘要。脚本核对 `SHA256SUMS.txt` 中的三类发行包。按提示亲自执行各场景，输入观察结果和非空日志或截图路径。结果写入独立运行目录的 `acceptance-summary.json`，并附带与摘要哈希一致的证据副本。跳过、失败、缺包、错包或缺证据均不通过。

到各渠道的首次启动检查时，先按该渠道的提示安装或解压候选包，并关闭已打开的 Remova。输入该渠道 `remova.exe` 的完整路径。脚本启动并关闭这个实例，记录实际进程路径、产品版本、SHA-256、进程 ID，以及通过 UI Automation 观察到可用的「软件」按钮。程序摘要须与同一渠道的独立解包结果一致。此后重新打开同一程序完成手工功能检查。

---

## 3. 凭据与授权验收要点

### 凭据绑定来源

| 步骤 | 预期 |
|---|---|
| 填入云端假 Key，保存 | 保存成功 |
| 切换提供商（不改 Key 输入） | 旧 Key 被清除，面板不再显示「已保存」 |
| 保存后切回原提供商 | 需要重新填写，不复用旧 Key |
| 填入 Key 后改为本地回环端点 | 请求中不出现旧云端 Key |
| 使用显式「清除」 | Key 被删除；重新分析不带鉴权头 |

### 重定向不转发凭据

在隔离 VM 中自行搭建回环服务，记录请求是否抵达以及请求头是否包含测试凭据：

| 场景 | 预期 |
|---|---|
| 301 / 302 / 303 同源 | 不自动跟随，或跟随后不带 `x-api-key` |
| 301 / 302 / 303 跨源 | 目标端点收不到 Key |
| HTTPS → HTTP 降级 | 拒绝，不发出请求 |
| 307 / 308 | 按实现拒绝或安全处理，日志无 Key |

### 孤儿授权分类

| 步骤 | 预期 |
|---|---|
| 正常扫描一个 shared 目录候选 | 该候选默认不勾选 |
| 用测试工具改写 IPC 请求，把 `shared` 置为 false | 执行被拒绝，且不发生任何删除 |
| 同样改写 `service_bound`、`kind` | 逐项被拒绝 |
| 未扫描过的新路径 | 被拒绝 |
| 扫描后取消，再请求该扫描的结果 | 被拒绝（不发布陈旧授权） |
| 普通非保护候选 | 正常清理成功（正例对照） |

提权测试分别验证 AI 配置的 DPAPI 失败路径与备份封印密钥不可读路径。AI 测试应制造用户凭据 DPAPI 操作失败，检查配置无明文凭据。备份测试应在 VM 快照内让备份封印密钥不可读并尝试恢复。两组用例各保留相应结果。

---

## 4. 安装与关键路径

按渠道各跑一遍，记录每一步的实际结果：

1. 全新安装 → 首次启动 → 应用列表可加载
2. 分析 → 预览（dry-run）→ 真实清理一个隔离 fixture 应用 → 备份 → 恢复
3. 目录迁移：计划 → 复制 → 切换 → 收尾；再对另一 fixture 走一次回滚
4. 升级：旧版 → 新版覆盖安装 → 数据与历史保留
5. 降级：新版 → 旧版，按 `RECOVERY.md` 步骤，记录是否可读旧数据
6. 卸载 → 残留检查

每个 fixture 必须自建、可清理，且不得指向真实用户目录或系统目录。

普通用户的 NSIS 路径还可用以下矩阵脚本复验。先用应用产生非空 AI 设置和测试清理历史，并关闭 Remova。候选 EXE 摘要取自同一批候选构建的主程序，例如便携包内的 `Remova.exe`。脚本断言实际 EXE 版本、候选 EXE SHA-256、安装位置、WebView2 导航是否就绪和夹具字节是否保留。

```powershell
pwsh -NoProfile -File scripts/acceptance/run-install-matrix.ps1 `
  -Installer140 D:\stable\Remova_1.4.0_x64-setup.exe `
  -Installer141 D:\artifacts\Remova_1.4.1_x64-setup.exe `
  -InstallDir D:\vm-fixture\Remova -CandidateExeSha256 "<候选 EXE SHA-256>" `
  -Out D:\acceptance\nsis-matrix -ConfirmReplaceInstalledApp
```

默认夹具为当前用户的 `Remova\ai-config.json` 和公共应用数据目录下的 `Remova\history.jsonl`。非默认配置可传 `-ConfigDir`，或用 `-FixtureFiles` 指向真实的配置和数据夹具。脚本在失败时也写出 `install-matrix.json`。导出证据后回滚虚拟机快照，以恢复试验前的安装、注册表和配置。

---

## 5. 性能与容量

阈值与命令见 [性能与容量阈值](../../benchmarks/performance-thresholds.md)。在同一机器和 WebView2 环境下测试已固定的候选。冷启动样本须来自三次不同 Windows 启动会话，并观察到软件导航控件就绪。最终判定要求六项指标、匹配的候选身份及阈值摘要。

---

## 6. 结果记录字段

每条记录至少包含：

- 检查项标识（凭据绑定 / 重定向 / 授权分类 / 安装矩阵 / 性能）
- 被测产物 SHA-256
- OS 版本、令牌类型（普通/管理员）、WebView2 版本
- 前置数据与 fixture 说明
- 步骤、预期、实际
- 日志或截图路径
- 负例是否真正到达目标分支
- 执行者与日期

---

## 7. 与报告的关系

完成本套件后，把结果回填到终审报告进度表对应行，并把状态改为「已完成✅️」；未执行的项目保持「未完成」。**不要**用本套件的存在替代执行证据。
