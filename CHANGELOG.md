# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.3.2] - 2026-10-21

本次发布汇总自 v1.3.1 以来的安全、正确性与可用性修复，以及专项扫描、深度卸载与性能改进。

### 新增（深度卸载能力）

- **损坏的安装记录（M1）**：安装目录已丢失但 Uninstall 记录仍在的 MSI 应用会在「更多」页单独列出，可直接重新运行官方卸载器并自动重扫；只读扫描，不自动删除任何记录。
- **服务绑定保护（M2）**：候选目录若承载已注册 Windows 服务的 ImagePath，会标记为 keep 类、默认不勾选，删除门禁直接拒绝——避免删掉仍在使用中的服务数据目录。
- **活跃孤儿保护（M2）**：孤儿目录近 3 天仍有写入时提升为高风险并注明「近期仍在写入」，不再作为普通残留展示。
- **空壳目录裁剪（M2）**：删除应用目录后会清理名称匹配产品/厂商且确为空的祖先目录（如 `Program Files\Vendor`），不越过已知根、不处理 reparse 点。
- **损坏记录清理（M2）**：在重新运行卸载器仍无效时，可二次确认后删除损坏记录；相关 Uninstall 键与 `Installer\Products` 孤儿键会先导出到安全备份，再按精确形状删除。
- **孤儿防火墙规则（M3）**：新增只读扫描与「孤儿网络规则」面板，列出程序文件已不存在的规则；删除需确认、服务端重新校验并先备份注册表值。
- **主目录点目录与 MuiCache（M4）**：补扫 `%USERPROFILE%\.<产品>` 及共享点根下的匹配子目录（共享根本身永不作为目标），并列出可执行文件已不存在的本应用 MuiCache 显示名缓存；MuiCache 只允许删除 `FriendlyAppName` 值。
- **锁占用诊断（M4）**：失败项现在可通过 Restart Manager 只读查询占用进程名与 PID，不再只能看到“文件被占用”。
- **只读属性处理（M4）**：删除树遇到 READONLY/HIDDEN/SYSTEM 属性时先清除属性再重试一次，不跟随 reparse 点。
- **报告总账与复核（M5）**：清理报告新增“实删 / 保留”体积总账，并提供“重新扫描该应用”（不预选任何新发现）与“深度核查注册表”（有界只读、仅报告不删除）；同名多安装记录会显示「多个安装」标记，但删除语义保持逐项独立。

### 安全

- **安装监视器不再把 Remova 自己的封印密钥当成安装残留。** `%ProgramData%\RemovaSealKeys`（`seal.key`、`initialized.v2`、`stage_*`）与 `%ProgramData%\Remova`、`%LOCALAPPDATA%\Remova` 一起整组排除，并按路径段大小写不敏感比较；此前一次点击即可删除 `seal.key`，让全部安全备份永久无法还原。
- **安装监视器对 `%APPDATA%` / `%ProgramData%` 下的新增项降为“疑似 / 中风险”**，永不预勾选。窗口期内 OneDrive、Teams、驱动更新等无关写入不再被标成“安装产物 · 低风险”。
- **共享运行时令牌改为按路径段锚定匹配。** `Community`、`Opportunity`、`Impunity`、`MontyPython`、`PhysXtreme`、`node.jsexport` 等名称不再被误判为共享组件而永久拒绝清理；Unity / Python / Visual C++ / DirectX / PhysX / JRE / JDK / WebView2 仍按共享处理。
- `parse_nl_intent` 与其他 AI 调用一致：自由文本先脱敏，路径形态 token 受 `allow_cloud_paths` 门禁。
- 还原点创建失败现在带上真实 Win32 错误码（系统还原被禁用、安全模式、拒绝访问等），用户也可在“更多”中关闭清理前创建还原点。

### 修复

- **无法解析的非空卸载命令改报 `had_command: true`**，中止后续删除。不再出现“程序仍登记在程序和功能里、文件已被删除、界面还弹成功”的组合。
- **清理结果据实判定成功。** `failed = 0` 但实际 `deleted = 0`（全部门禁跳过）不再显示成功；重试也只在确实删掉内容时提示成功。
- **PATH 还原逐条聚合错误并继续导入注册表。** 单个坏条目不再造成“文件已写回、注册表永不导入、真实错误被丢弃”。
- **安全仓库只列出有有效封印的会话。** 备份中止留下的空目录不再被列为可还原；会话信息增加“未封印”提示，恢复选择器过滤这些目录。
- **修复安全仓库显示 0 KB。** 目录体积改用不依赖线程局部扫描预算的有界遍历，线程复用后不再把所有会话显示成 0 KB。
- **Yarn 1 与 Yarn Berry 不再共用 `YARN_CACHE_FOLDER`。** 设置该变量后两行不再折叠成同一路径（重复行、假计数、必然的“路径已消失”）；Berry 改用 `YARN_GLOBAL_FOLDER`。
- 备份文件名按 Windows 组件上限截断（最终口径见下方「第 38 轮评审收尾」条目）；长文件名残留不再让整次清理因备份失败而中止。
- 路径含字面 `%`（如 `50% Off`）不再被永久判定为危险 PATH 项，可正常清理与还原。
- **目录守卫不再往 `C:\` / `%ProgramData%` 写标记。** 只在本次操作的目标目录创建 delete-on-close 标记，也不钉住真实子项阻塞正常删除。
- **扫描失败/取消后专项面板不再永久停在“正在分析各项占用…”**；清理 IPC 失败时不再把上一次报告和可点的重试按钮留在屏上。
- **专项面板的可读性与一致性：** 扫描总量与已选量分列；更新缓存行不再自称“疑似残留”；组头使用“安装包文件 / 更新程序缓存”；组间也按体积降序；“全选”与孤儿页一致排除保留项；长路径可复制；扫描完成后持久显示“上次扫描：刚刚 · N”。
- **`omitted` 统一为“未显示的行数”**，PE 预算未检查项改在“无法完整校验”里说明；孤儿扫描与闲置雷达也显示“另有 N 项未显示”。
- 扫描降级原因改为稳定代码并在前端本地化，中文界面不再出现 `USERPROFILE is not set` 等英文句子。
- 实时区域改为稳定挂载并节流播报，勾选几十行不再重复朗读；扫描进度阶段不再每 250ms 打断朗读。
- 安装后的 AI 风险简报返回空结果时，确认对话框明确说明“简报不可用”，不再静默缺失 AI 行。

### 修复（第 38 轮评审收尾）

- 备份文件名按 UTF-16 单元预算截断（digest 前缀后 238 单元）：Windows 的组件上限按 UTF-16 计数，此前按字符数截断时 emoji 等字符组成的超长文件名仍会超过 255 单元、让整次清理因备份失败而中止。
- 取消扫描后体积估算立即停止：并行的体积估算工作线程此前看不到线程局部的任务上下文，取消后仍会跑满整个条目预算；估算阶段在进度条上也不再回退为「准备扫描」文案。
- 防交换守卫加固：目录守卫标记在名称碰撞时自动重试，且绝不写入盘根目录；标记仍只落在本次操作的目标目录，祖先链（含 `C:\`、`%ProgramData%`）保持无标记。
- 孤儿扫描的截断提示改为如实口径：「另有 N 个候选目录未显示」——上限在目录形态判定之前施加，被截断的是候选而非确认的残留。
- 备份的原子临时文件改用与目标名无关的短名称：此前在长文件名后追加 `.<pid>.<nonce>.tmp`，会让 224–238 个 UTF-16 单元的文件名再次超出 255 单元上限而中止整次清理；238 字符名与 emoji 名现已端到端验证可备份。
- 超过 260 字符的长路径：目录钉住句柄改用扩展长度（verbatim）路径打开，长名目录的备份与还原不再因 Win32 MAX_PATH 中止。
- 取消扫描时，体积估算进度按已完成的行数上报，不再在取消瞬间显示 100%；并行估算线程现在能通过取消令牌即时停止派发后续行。
- 闲置软件雷达的安装日期与「安装日期」标签显示在同一行，不再被拆成两行。

### 改进

- **体积估算按行分配子配额。** 同一预算下后到的行也能拿到体积下限，专项面板不再约 58% 的体积列显示“—”；后台 worker 显式标记，前台遍历保留槽不被扫描自己占满。
- 目录枚举减少重复 stat（下载、更新器、共享缓存、孤儿、磁盘雷达等），并去掉遍历预算的每次 `Arc` 克隆。
- 并行体积测试夹具改用小预算 + 小树，测试文件数从约 79k 降到约 800。
- 文档与门禁：架构文档改为单次删除判定与新返回类型、用户指南补充截断说明（以上为本机 `docs/` 文档，不入库，无法被 CI 校验）；`check-reviews-index` 与 `check-commands` 补回归测试，命令门禁不再重复输出 OK。

以下为 1.3.2 早期批次（安装包与更新缓存扫描）的变更：

安装包与更新缓存扫描补上常见的更新器目录形态。

### 修复

- 安装包与更新缓存扫描现在会识别 `%LOCALAPPDATA%` 下直接以 `<产品名>-updater` 结尾的一级更新器目录（如 `obsidian-updater`、`token-monitor-updater`）；此前只会在这些目录内部查找固定名称的缓存子目录，因此整类目录不会出现在扫描结果中。名称匹配仍按中风险候选处理，不会默认勾选，清理目标依旧必须来自本次扫描的服务端授权清单。
- 修正更新缓存在界面上被显示为通用「配置文件」的问题。
- 修正专项缓存清理中 pnpm 的路径规则：原规则查找的 `%LOCALAPPDATA%\pnpm\cache` 不是任何 pnpm 版本实际使用的目录，该规则从未生效；改为覆盖 `pnpm-cache` 与 `pnpm\store` 等已知布局。
- 修正扫描结果的顺序依赖：更新器目录、下载安装包与共享缓存子目录此前都在枚举过程中设上限，命中哪一批取决于文件系统顺序，因此不同机器、同一机器不同时间的结果会不同。现已改为完整枚举后统一排序再截断。
- 修正 Gradle 缓存路径：其用户主目录是 `~/.gradle`，不是 `%LOCALAPPDATA%\gradle`。

### 新增

- 专项缓存清理补充 uv、Bun、Go module、Maven、NuGet packages 与 VS Code 的 `Cache` / `CachedData` 缓存目录。
- 对多个工具共用的缓存根目录改为**逐项列出**：`~/.cache` 与 `%LOCALAPPDATA%\JetBrains` 下的每个子目录单独成为一项，共享根目录本身不会作为清理目标。`~/.cache` 子项与 JetBrains 下非 IDE 目录（如下载的发行包）按中风险处理，默认不勾选。
- JetBrains IDE 目录只列出 `caches`、`compile-server`、`log`、`tmp` 这几个确认可弃的子目录；设置、插件与扩展目录一律不列入。
- 下载目录中的安装包识别改为不依赖文件名：除安装包扩展名与中英文安装名称外，还会校验 `.exe` 的 PE 头，因此以产品名命名、不含任何安装关键词的安装器（如 `python-3.12.1-amd64.exe`、`Git-2.43.0-64-bit.exe`）也能被发现。这类"确认是可执行文件但无法确认是安装器"的条目标为「可能是安装包」，默认不勾选。
- 安装名称关键词补充中文（`安装` / `安裝`），并排除 `uninstall.exe` 这类含 install 却不是安装包的文件。
- 下载目录位置改用系统「下载」Known Folder，用户把下载文件夹移动到其他盘符时不再漏扫。
- 归档类（`.zip` / `.7z` / `.rar` / `.tar.gz`）在名称含安装特征时也会列出，但标为「可能是安装包」、默认不勾选——可便携版软件常以压缩包分发，名称本身不能证明整个文件可弃。
- 小于 8 MB 的 `.msi` 等安装包此前会被体积门槛跳过，现已改为仅对上述"可能是安装包"两类应用体积下限，专用安装包扩展名不受体积限制。
- 专项缓存清理尊重各工具自带的缓存重定位变量：`CARGO_HOME`、`GOPATH`、`PIP_CACHE_DIR`、`NUGET_PACKAGES`、`GRADLE_USER_HOME`、`UV_CACHE_DIR`、`PNPM_HOME`、npm 缓存配置与 `XDG_CACHE_HOME`。把缓存改到其他盘符的用户此前会得到空结果。
- 编辑器缓存改按目录结构识别：`%APPDATA%` 下同时存在 `Cache` 与 `CachedData` 的目录即视为该系列编辑器，因此 VS Code、Insiders、VSCodium、Cursor、Windsurf 等分支都覆盖，无需逐一列名。补充 Yarn 2+ 的 `~/.yarn/berry/cache`。
- 浏览器缓存覆盖全部配置文件（`Default` 与 `Profile N`）与三个可弃缓存目录（`Cache` / `Code Cache` / `GPUCache`），产品覆盖 Chrome、Edge、Brave、Vivaldi、Chromium 与 Opera / Opera GX。此前只查 Chrome 与 Edge 的 `Default` 配置的单个 `Cache` 目录。

### 修复（发布流水线与测试基线）

- **重新打开并修好发布流水线。** CI 与 Release 两个工作流在远端被手动禁用已久，v1.3.0 与 v1.3.1 均没有任何对应的 Release 运行记录，全部质量门禁形同虚设。现两个工作流均已恢复启用，并补上双向闭环：CI 运行时校验 Release 未被禁用，标签发布时校验该提交的 CI 确实通过（没有运行记录同样拒绝发布），避免"门禁已配置但被关掉"再次无人察觉。
- **修复一个让全新克隆根本无法编译测试树的问题。** `src-tauri/src/path_value_smoke.rs` 与 `pipeline_smoke.rs` 被 `.gitignore` 忽略，却被 `lib.rs` 以 `#[cfg(test)] mod` 声明，因此干净 checkout 下 `cargo fmt --check`、`clippy` 与 `cargo test` 都会在解析模块时直接失败——此前本机跑通的测试数量只存在于开发机上。两个文件现已纳入版本控制。
- **修复工具链兼容性。** `scan_task.rs` 使用的 `AtomicU32::fetch_update` 在较新的 Rust 工具链上已被废弃，在 `-D warnings` 下是硬错误（本机通过、CI 运行器失败）。改用 `compare_exchange`，行为不变。
- **修复删除门禁的二次判定。** 备份阶段与删除阶段原先各自调用一次清理门禁，中间夹着最长 300 秒的官方卸载器，而门禁会读取忽略列表与可变的扫描授权清单——备份时被拒的条目在删除时可能被放行，从而在没有备份的情况下被删除。现改为整个清理流程只判定一次，备份集与删除集由构造保证一致；删除级安全校验仍然对每个条目执行。
- **修正注册表读取失败与"值不存在"的混同。** 访问被拒、数据超出缓冲等读失败此前一律被报成"值不存在"，导致安全仓库导出的 `.reg` 文件静默漏值——清理之后没有还原来源，界面却报告备份成功。现在只有"未找到"算缺失，其余一律报错。大于 1 MB 的注册表值将导致备份明确失败并告知用户，而不再是静默无备份。
- **卸载器已删除的注册表项改记为「已跳过」而非「失败」**，与文件/目录项的"路径不存在"以及预览阶段的口径一致。
- **修正安装监视器的漏报。** 快照根目录原先不包含 `%APPDATA%` 与 `%ProgramData%`，这类安装得到空差异却告知用户扫描完成。补齐根目录的同时关闭一个因此暴露的安全问题：Remova 自己的快照文件会出现在差异里，监视器此前会提议删除自己的基线。
- **统一路径规范化。** 授权清单、PATH 条目比较、孤立残留与安全红线原先各有一份实现，其中三份完全相同且都不处理 Win32 按路径段去除尾随点与空格的规则。表现为扫到的 `C:FooBar.` 存入键 `c:ooar.`，删除 `C:FooBar` 的请求因此被拒——用户看到一个删不掉的行。现统一为 `fsutil::path_key` 单一实现，无法命名的路径一律不授权。
- **修正扫描结果被静默截断。** 专项缓存在每个共享根目录下有 60 条上限，浏览器与编辑器的每个配置目录也有上限，超出部分此前无声消失，界面仍说"扫描完成，发现 N 项"。现在后端报告未显示的条目数，前端显示"另有 N 项未显示"；列表按类别分组、组内按体积从大到小排列，体积最大的缓存不再排在一堆 KB 级目录后面。
- **区分"确实没有"与"读不到"。** 扫描根目录读不到时（权限受限、域管机器）此前返回空结果且无任何提示，用户会看到"扫描完成，发现 0 项"从而误判电脑很干净。现在逐条报告读不到的原因，空态按"没有可清理项"与"以下位置无法读取"分别提示。
- **修正估算阶段的进度停滞。** 体积估算内部此前不汇报进度，而这是大型缓存场景下最长的一个阶段，界面会长时间停在"估算中"。现按固定步长汇报已测量条目数。
- **修正若干界面问题**：清理结果从"3–7 秒的提示"改为就地展示失败项的路径、原因、查看位置与重试；确认对话框的条目计数不再重复；补上"疑似/共享根子项"这一类风险说明；AI 风险评估失败不再静默（此前用户会以为 AI 已评估过）；安装日期解析改用统一实现（`2024/01/15` 这类格式此前会静默失配，且 `2025-13-01` 会被悄悄算成 2026-01-01）；专项缓存行的证据说明补上缺失的本地化；600ms 长按确认补上键盘操作路径（此前键盘轻敲完全无效）；导出报告的说明从"分析报告"改为"清理结果"。
- **统一测试夹具。** 新增 `test_support` 模块：临时目录改为 RAII（断言失败也清理，此前每个失败的测试都会在 `%TEMP%` 留下垃圾）、环境变量覆写自动恢复、一把全局锁统一守护进程级环境改写。迁移过程中修掉两个真实的测试竞态：多处直接改写 `REMOVA_BACKUP_DIR` 且只删不恢复，以及夹具的锁先于环境恢复而释放——后者只在真正用过本产品的机器上才复现。

### 改进

- 体积估算改为多线程并行（最多 3 个 worker 共享同一个条目预算，预算不会被乘倍），并增加全局遍历信号量，前台请求不会被一批后台扫描堵住。
- 下载目录扫描每条目的 stat 次数从 3 次降到 0 次（改用目录枚举缓冲区里的文件类型），安装包名称匹配复用已分配的小写串。
- 前端：体积估算进度改为 120 ms 合并发布；`App.tsx` 的 flow/residual 对象改为 memo（另修掉一处同类问题的内联 JSX 依赖）；列表视口测量的 MutationObserver 不再监听字符变化、padding 改为按需读取；软件列表排序改为 decorate-sort-undecorate（推荐分从约 350 次正则降到 60 次）；管理列表行的长文本不再溢出到相邻行。
- 文档：架构文档补上 8 个缺失模块与"新增一条扫描规则的完整触点"清单；评审索引改为可校验（新增 `scripts/check-reviews-index.mjs`）；产品文档的版本基线统一并删除会随发版失效的表述；删除冗余的 release-notes 目录（内容已全部在 CHANGELOG 中）。
- `CONTRIBUTING.md` 中对 ignored 测试的承诺改为如实描述：逐个标注"CI 自动 / 手动"以及它到底改动什么。把 7 个测试全部接进 CI 会把红灯引进门禁——其中两个会创建真实系统还原点、写删真实的注册表 Run 值，另一个断言本机存在文件夹启动项批准值。

## [1.3.1] - 2026-10-10

本版本提供 Windows x64 安装包与便携版，采用手动下载更新；发布产物未签名。实际升级、提权、备份还原、签名更新及 Windows 高 DPI 环境验收仍待完成。

### 改进

- 孤儿残留页及分组选择按钮统一为「全选」，合并可清理与需确认项入口，保留项不自动勾选；拉开组标题与条目间距，判定依据使用轻量按钮，减少右侧操作堆叠。
- 残留列表支持全选/取消当前筛选结果，保留隐藏已选项；点击行内路径或空白可切换勾选，判定依据与复选框保持独立，扫描中禁用选择。
- 关联清理主按钮按执行策略显示「卸载并清理」或「清理选中项」，同步说明官方卸载与勾选关联项的操作范围。
- 正常关联清理默认先运行官方卸载器；跳过卸载的直接清理移入高级选项，并提示可能留下安装状态。新扫描恢复默认策略，卸载后的残留与安装追踪、孤立残留不重复运行官方卸载。
- 将「仅预览」移入「范围与备份说明」并改名为「检查清理计划」，说明检查范围与实际清理的区别，让主要操作更简洁。
- 软件关联分析、孤立残留与体检、安装包和专项缓存、磁盘与闲置雷达、启动项/服务/计划任务列表及安装追踪显示扫描阶段和已发现数量，支持只读扫描取消；未知总量使用不定进度。
- 取消扫描明确标注结果不完整；在途读取返回后停止后续工作，重新扫描清空旧目标，迟到结果与路径授权不能覆盖新任务。已有批量清理停止仍保持「当前应用完成后停止」。
- 清理结果分开展示已删除、失败、跳过和等待重启；部分失败、延迟删除及中止不再呈现为全部清理成功。
- 清理详情展示常见失败原因、完整路径和原始诊断；失败项支持分页与安全的文件定位，路径无法打开时显示原因。
- 清理确认实时展示是否创建备份及相应后果，默认仍不勾选；无有效备份不承诺可恢复，备份失败继续中止清理。
- 有效备份结果可进入本次对应会话的还原预览；会话缺失时明确提示，不回退到其他备份。已有还原预览优先展示冲突、缺失和受保护目标，保留执行前重校验。
- 残留列表区分当前可见、全部已选和隐藏已选项目，新增可叠加类型和风险筛选的「只看已选」；取消最后一项后保留返回全部结果的入口。
- 详情返回及刷新保留本次运行的搜索与排序，同时清除过期扫描和清理选择，不跨启动恢复删除授权。
- 安装追踪快照期间禁用重复操作；开始取消不标记为追踪中，结束取消保留会话和结束重试入口。结束快照失败时保留会话供重试，基线缺失才回到开始；失败或取消不生成清理清单。

### 修复

- 修复多词软件完整名称与关联路径分隔符不一致、自身无花括号卸载注册项匹配失败造成的跳过；通用名称和其他软件路径继续拦截，用户数据、共享组件及路径/注册表安全检查不变。
- 清理报告默认收起详细内容，摘要保留处理数量；展开明细限制高度，残留列表随外层滚动重新计算可见空间，减少空白与过小的滚动区域。
- 保留卸载清理报告并滚入可视区，明确官方卸载结果与复扫后的剩余关联项；复扫不再清空报告或自动勾选目标，清理前的旧列表与选择失效，后续残留清理不重复运行卸载器。
- 修复软件页操作指引的重复步骤编号，精简中英文说明，明确官方卸载、残留核对及可选备份的流程。
- 历史记录区分加载、读取失败和真实空记录，失败可重试；关闭面板后的迟到读取不再重开或覆盖当前状态。
- 修复孤立残留页面首次自动扫描未启动及旧任务结束回调干扰新任务状态的问题；管理列表换页、刷新后的迟到结果与错误不再覆盖当前页面。
- 修复深色导航背景与扫描状态语言切换，改善窄窗口下的长名称、长路径和弹窗滚动。
- 打开工具面板自动滚入可视区并转移键盘焦点；雷达扫描与停止状态统一由进度栏展示，减少重复提示。
- 修复软件、启动项、服务、计划任务及孤立残留列表区域的滚轮死区：长列表按工具栏下方实际剩余空间铺满，列表到达边界后允许继续滚动外层页面。
- 对齐残留列表的判定依据图标，补齐普通关联扫描证据的中英文文案，保留路径、注册项名称和风险信息；切换语言后再次查看依据使用当前语言。

## [1.3.0] - 2026-10-05

安全与正确性修复、安全仓还原预览与深度卸载流程打磨、界面精简与主题统一。

### 新增

- 安全仓还原预览：应用还原前逐项标注目标状态（可还原 / 已存在 / 备份缺失 / 受保护不可用，超过 200 条如实标注截断），确认消息汇总冲突与不可用数量，确认前自动重新校验
- 清理确认前展示清理范围与已知占用估算，附备份指引与「仅预览」入口

### 安全

- 安全备份升级为 RSEAL2：CNG HMAC-SHA256 + 当前用户 DPAPI，32 字节随机密钥存放在 Known Folder ProgramData 下仅 SYSTEM/Administrators 可访问的目录；拒绝弱 ACL、链接、丢失密钥或初始化状态异常，备份与还原需要管理员权限
- 旧 RSEAL1 / 缺失封印不再自动还原，仅保留查看与手工导出；新备份先在特权私有 staging 捕获可信输入，再发布封印，拒绝追加或重新签署既有快照
- 文件目标映射、PATH 快照和注册表还原字节在所有写入前验证；注册表导入临时文件与全部祖先目录同时钉扎，并通过目录句柄持有子项或自动清理的防护文件，阻止原地转换为 junction 和重定向 staging 写入；单值备份只授权 value.reg，删除该文件不能回退到导入整键；普通文件内容完整性和同一会话的历史状态防回滚不在本次封印范围

- 安装监控记录的注册表值改用与清理管线一致的「键|值」编码：监控到的 Run 自启动项现在可以被真正删除
- 用户资料红线（文档/下载/桌面等库根）与同步冲突目录按 Win32 规则逐段规范化匹配，尾缀点/空格（如 `Documents.`）无法再绕过
- 还原写回先钉住目标父目录、按句柄解析真实路径并重跑门禁：目标路径上方被植入目录联接也无法把还原导向受保护位置；叶子本身是 reparse 点则拒绝写穿
- `.reg` 还原导入把 `"Name"=-` / `@=-` 识别为值删除并拒绝；以 `=-` 结尾的普通引号写入不受影响
- 禁用 RunOnce 启动项不再假报成功——RunOnce 没有禁用机制，改为明确报错并建议直接删除
- AI 配置迁移失败不再静默：磁盘上的明文 Key 会被抹除，恢复后需重新输入；配置写入改为临时文件+改名，中断不再损坏密文
- 导出的清理历史 CSV 对 `= + - @` 开头的单元格做公式中和，防电子表格公式注入
- 图标缓存读取校验 PNG 魔数、写入改为原子替换：损坏文件不会被当作图标输出
- 删除门禁拒绝空服务名的「Services|值」形态
- PATH 还原与删除同一门禁:危险条目拒绝写回;系统 PATH 仅在备份快照含实证时恢复
- PATH 修改按注册表原始字节与类型回写：REG_EXPAND_SZ 的 `%VAR%` 段（如 `%SystemRoot%`）不再被展开固化，值类型不再被降级为 REG_SZ
- 目录复制的子文件改用不跟随链接的复制原语,检查与复制之间换入的联接不再被跟随
- 还原目标门禁拒绝相对路径与 UNC 形态(含盘符相对的 `\srv` 形式)
- 明文 http 仅环回的判定改为解析四段 IP,`127.0.0.1.evil.com` 不再被前缀放行
- 批量白名单建议在写入前整批校验长度、控制字符与规则种类，非法项不会留下部分规则

### 正确性

- 安装位置关联（App Paths / Run 启动值 / PATH 条目 / 服务 ImagePath）要求路径段边界：`C:\Steam` 不再认领 `C:\SteamTools` 的残留
- Common Files 厂商目录关联同样要求安装位置是合法安装根：盘符根或系统浅根（如 `C:\`、`C:\Windows`）不再能借前缀认领该盘全部 Common Files 厂商目录
- 安装监控快照遍历改按 reparse 属性跳过目录联接/云占位目录，并新增 64 层深度上限，不再依赖随 Rust 版本变化的 symlink 判定
- 在线更新入口判定加卸载器 NSIS 特征校验；检查更新时的安装包按架构与版本号锚定匹配，不再命中任意 `*-setup.exe`（在线更新载荷本就经签名校验，此项防误判）
- 在线更新失败提示改为可读的本地化文案并区分原因（占用/版本变化/检查失败），不再把内部错误串直接透出
- 手动下载入口在安装包链接打不开时回退到 Releases 页面，不再重复打开同一条死链
- 体积估算遇到在线更新占用或读取失败时不再把 0 KB 冻结进缓存，下次刷新会重新估算
- 备份目录名全程携带摘要后缀：字面 `C__Tools` 键不再与折叠的 `C:\Tools` 共用目录，旧备份会话恢复不受影响
- 安装监控快照在遍历被预算截断时降级：只展示差异、不再据此武装删除白名单，避免把本就存在的条目当成新增而放行删除
- 策略 Explorer\Run 条目禁用时明确报错：策略键不受禁用开关管辖，旧版会假报成功而条目照常运行
- 删除门禁与恢复门禁共享任意盘符系统根保护：`D:\Windows\...` 形态不再因环境根仅覆盖 C 盘而漏判
- `sc delete` 以 Services 段后的名称为准，`Services\Xxx\Parameters` 子键不会被误当服务名执行
- 注册表写原语对 Services 键只允许 `Start` 值名，防植入型 `ServiceDll` 被改写
- 更新进度遮罩对齐全站对话框交互（焦点圈定/恢复），屏幕阅读器按 10% 阶梯播报下载进度
- TEMP 残留只匹配条目名，不再因用户名/上级目录含产品词而误报整个目录
- 注册表默认值只接受字符串类型，DWORD 不再被解码成乱码参与匹配
- 扫描结果携带稳定的应用标识（来源+注册表键+名称），同名不同源的软件扫描结果不再互相串台
- 盘面统计跳过目录联接，不再跨卷重复计数；深度截断的目录大小如实标记为下限
- 打开外部链接支持方括号 IPv6 写法（`[::1]` 回环放行恢复生效）
- 共享 GUID 提取扫描全部花括号组并校验 36 位十六进制形状
- 修复任务管理器「启动应用」中文件夹条目的禁用:此前按文件主名写入 Explorer 不读取的值名,禁用假成功;启用状态读取同样按完整文件名对齐
- 修复服务/驱动 ImagePath 关联通道:此前把值当子键读取,安装位置关联从未命中(纯漏报)
- 修复 AI 逐项解释在 AppData 多段路径下被静默丢弃的问题(路径脱敏非幂等导致回配失败)
- 修复软件名含 Unicode 特殊检字(如 U+212A)时切片越界导致扫描崩溃的问题
- 孤儿扫描跳过 Temp/CrashDumps/SquirrelTemp 等通用缓存目录;损坏的安装位置(仅盘符根)不再导致整盘路径被误判为"已安装"
- 修复中文名称边界匹配和空出版商词元导致的扫描崩溃；跨盘证据权重与得分保持一致
- AI 解释按请求上限分批处理全部候选，尾批结果也能正确回配与复用缓存
- 图标缓存临时文件在失败时清理，并参与过期清理；闲置雷达的截断尺寸显示为下限

- 提权重启握手重做：关键任务进行中先完成退出确认再触发 UAC；提权副本等待旧实例完全退出后接管，交接不再受确认耗时影响
- 更新检查区分失败原因：网络不可达 / GitHub 限流 / 仓库暂无发布版本各有明确文案，不再一律报「检查更新失败」
- 还原预览对无法读取的目标降级标注而非整体失败；文件名中包含 `..`（如 `a..b.txt`）不再导致整个备份无法还原
- 残留列表滚轮在列表与页面间自然衔接；超长注册表路径按列宽截断显示，不再压到相邻列
- 判定依据面板打开时自动滚入视野，再次点击同一行可收起
- 详情抽屉「查看详情」始终有响应；深度卸载前的关联分析不再被称作「残留」
- 应用图标背景透明化，深色主题下不再带白色底色

### 体验

- 每个图标关闭/取消按钮都有可读名称；应用详情「⋯」菜单支持方向键/Home/End 漫游，打开即落在首项
- 强制清理期间页面按钮随忙碌态禁用
- 还原/白名单面板加载中有提示，不再闪现空态；盘面/闲置面板有专属文案；「搜索无结果」与「暂无记录」分开表述
- 批量卸载按钮与备份横幅改用暗色墨水，暗色主题下对比度达标
- 残留列表不再因滚动边距双重扣减裁掉末行；工具箱扫描结果改用虚拟滚动
- 新增文字级 warn/ok 色令牌:浅色主题下小号警示/状态文字对比度达标;孤儿清理危险按钮改用深墨
- 行内「⋯」菜单与应用详情菜单同源:方向键/Home/End 漫游、打开即聚焦、Esc 返回触发钮
- 切换清理报告不再残留上一次的"清理后复核"清单与 AI 解读;卸载后自动重扫不再重跑官方卸载器
- 历史搜索、列表搜索、AI 开关等控件补齐可访问名称
- 多项对比度与可访问性打磨:占位符文字、小号状态徽章、Toast 播报方式、截断名称提示、开关类按钮的按压状态
- 扫描误报边界进一步收紧:Run/驱动/孤儿名称匹配要求词边界,CLSID 枚举去重,快捷方式探测支持混合大小写与 UTF-16 目标
- 界面文案清理 2 个失效键
- 手动停止尺寸估算后，列表刷新不会重新启动已放弃的队列；共享强调徽章使用文字对比度令牌

- 扫描清理区精简为「操作栏 + 筛选行」两层：选中数、预计释放与需留意数量集中在操作栏，范围与备份说明收进折叠，确认与保留桶改为就地筛选芯片
- 残留类型标签（目录 / 注册表 / 文件）跟随界面语言
- 残留扫描标题改为「可能的残留：共 N 项 · 已确认 N 项」；进入扫描后页脚不再显示软件列表的选中统计
- 「打开 Releases」入口移到「系统与帮助」分区标题右侧
- 点击管理员徽标提权前增加确认说明；未启用智能解释时入口收为操作栏按钮
- 明暗两套主题下导航、工具卡、列表、面板间距与状态样式统一打磨

### 工程

- 版本三源对齐 **1.3.0**
- 贡献文档的 PR 检查单统一指向覆盖率门禁（`npm run coverage`）
- App 纳入覆盖率门禁，死 i18n 键通过只读检查接入 CI；注册表导出测试必须实际生成内容才通过
- 文件换链回归作为明确要求 symlink 权限的专项，在 CI 和发布流程中单独执行

- 全局样式改为打包样式表分发，移除运行时 `<style>` 注入（规避 CSP 对内联样式的限制）

## [1.2.1] - 2026-09-24

安全与正确性修复。

### 安全

- 备份还原写回**每条**目标都要求本机封印（path_map seal）；缺封印或目标被篡改则跳过并提示，不再具备「改会话文件 → 写任意路径」的面
- 受保护路径（Program Files / Windows / 库根 / 同步冲突等）还原改为**跳过并警告**，不再中止整次还原，混合会话可部分恢复
- 备份/复制拒绝跟随目录联接（junction）等 reparse 点，与删除路径同一门禁
- 目录删除逐级句柄钉住（不共享 DELETE 权限）+ 句柄级 reparse 校验：删除期间路径无法被换入联接，同用户进程的 TOCTOU 窗口关闭
- 注册表值写入原语（新建 / 写二进制 / 改名 / 服务 Start）内置目标白名单：仅允许 Uninstall 根、Run/StartupApproved、Services、Remova 自有键
- 注册表导出对 DWORD/QWORD 解析失败直接报错，不再静默写 0
- 扫描长注册表值（如超长卸载串）按系统上报长度重试读取，不再静默截断
- 会话目录大小改用有界、reparse 安全的遍历；同一秒内创建的会话自动加后缀防合并
- AI Key 全链路内存清零（zeroize）：配置对象、请求头副本退出作用域即抹除；磁盘上仍为 DPAPI 加密
- AI 错误保留稳定码并透传具体原因（未配 Key / 未配模型 / 连接失败 / 空响应 / 解析失败各有文案）
- `http://` 明文远程地址仅允许环回，远程一律要求 https

### 体验

- 「更多」页安装追踪入口可打开监控面板（不再只在有变更时出现）；空态有说明
- 扫描切换后 AI 解释忙碌态不会卡死；列表刷新会裁剪已消失的选中项
- 安装监控差异如实标注截断数量（「另有 N 条未展示」）；监控项带用户数据/库红线标注
- 孤儿结果分组超过 8 条可展开/收起，不再不可达
- 软件列表表头排序支持键盘（Enter/Space）与 `aria-sort`
- Copilot 自然语言直达按钮显式声明作用范围（「批量卸载这 N 项」「强制清理"某应用"」）
- 清理 78 个未使用的界面文案键，中英文案保持一一对应

### 工程

- 版本三源对齐 **1.2.1**；`ARCHITECTURE` 命令表与 `generate_handler!` 对齐
- 单元测试纳入公开仓跟踪，CI/`npm test` 有真实信号
- IPC 契约测试升级：前端 `invoke()` 与 Rust `generate_handler!` 注册表双向对拍
- 计划任务启用状态反转为「仅已知就绪/运行标记」，未识别语言不再误报启用
- TypeScript 开启 `noUncheckedIndexedAccess`（44 处索引访问显式判空）
- vitest 覆盖率门禁（逻辑层 lib+hooks）接入 CI；新增启动竞态 / 软件控制器 / 编排钩子测试
- CI：package 依赖 test、release 增加 check-commands 与二进制冒烟；发版脚本兼容 `## [vX.Y.Z]` 标题

## [1.2.0] - 2026-09-24

工具箱增强与图标统一。相对 1.1.1 为功能版本。

### 新功能

- **清理历史可删了**：历史面板支持删除单条记录，也可一键清空；只动历史日志，不影响备份
- **闲置软件雷达**：找出长期未动、占用偏大的软件（只作建议，卸载仍在软件列表完成）
- **安装包与更新缓存**：清理下载的安装包与更新缓存；每条可「查看位置」，默认不勾选用户库文件
- **磁盘占用雷达**：只读查看哪些目录占空间，可下钻与打开位置
- **专项缓存清理**：开发 / 游戏 / 浏览器工具链缓存（如 npm、Steam 着色器、浏览器 Cache）；不碰存档与浏览器配置

### 体验

- 工具卡、侧栏、管理页签与列表类型图标统一为清晰描边风格
- 任务栏与托盘图标更清晰；软件列表图标在更新后会自动刷新，不再停在旧图
- 清理列表每行可打开所在文件夹

### 修复与安全

- 安装包扫描更克制：不再把普通压缩包或无关 exe 当安装包
- 闲置判定更准：安装很久但最近仍在写入的软件不会被误判为闲置
- 磁盘下钻限制在系统常用目录；专项/安装包清理只允许删除本轮扫描到的路径
- 卸载命令只执行本次扫描登记过的条目，杜绝伪造卸载命令执行任意程序
- 安装路径关联更稳：用户目录等系统浅层根不当成软件目录；`C:\Steam` 这类便携安装根仍可关联
- 还原备份可写回 `Documents\<应用>` 等库子路径（库根 / 开机启动项 / 同步冲突仍禁止）；会话删除按名称可正常操作
- 删除残留时拒绝目录联接/符号链接，避免被中途换路径
- 界面确认支持键盘长按；删除备份前会二次确认
- 清理历史读写加锁，超长日志自动压缩；同内容多条记录可逐条删除；若干错误提示不再暴露系统内部细节
- 备份还原写回以本机用户绑定的校验记录登记，防止被改写的目标路径；库根与开机启动项仍禁止写入
- 异常编码路径不再进入扫描与删除候选；清理历史读取加上限，异常大日志先压缩再读

## [1.1.1] - 2026-09-21

稳定性、安全纵深与发布可靠性补丁。**无新功能**，主流程与 1.1.0 相同。

### 亮点

- **分析结果更稳**：快速切换软件时，残留列表、勾选和智能说明不会再被上一次扫描覆盖；导出的 HTML 报告始终对应你正在看的软件
- **大列表更顺**：软件列表与残留列表按实际行高对齐，长列表滚到底不再错位或裁剪；估算体积时操作更跟手
- **删除更稳**：用户库根目录 / 同步冲突永不误删；库内应用数据与 `AppData` 更新包可关联清理（默认需确认）
- **安装包发布更可靠**：安装产物缺失时发布会直接失败，不会再出现「有版本页、没有安装包」

### 行为变化（相对 1.1.0）

删除面更保守的同时，**应用数据仍可清干净**：

1. **用户库「根目录」永不删除**  
   `Documents` / `Downloads` / `桌面` 等库**根本身**以及同步冲突目录，任何删除入口都会拒绝并记为跳过。  
   **库下面的软件子目录**（如 `Documents\某软件\`、`Downloads\安装包.msi`）在与软件关联后**可以清理**，但默认不勾选，需你确认（可能含存档/个人文件）。

2. **Common Files 只认厂商目录段**  
   位于 `Common Files` 下的残留，只有当**厂商目录名**与软件安装名 / 发布者匹配时才会关联可清；任意子串命中不再算关联。`Common Files` 等共享根目录仍然禁止清理。  
   `AppData` 下的缓存、更新安装包（含 updater 目录）**不受此影响**，仍按关联正常清理。

3. **智能说明宁缺毋错**  
   路径脱敏后若无法唯一对应到某条残留，该项不再显示智能说明（而不是显示可能错配的说明）。

### 修复

- 深度分析 / 智能说明 / 清理复核的并发竞态：慢返回不会覆盖当前软件的结果，加载态只由最新请求结束
- 软件列表滚动对齐与残留列表裁剪；体积估算期间整页高频重刷
- 「更多」页导出的 HTML 报告可能描述成另一台软件
- 体积估算角标可能一直停在「估算中」
- 含 `""` 或 `\"` 的卸载命令行拆参错误，官方卸载器可能拉起失败
- HTML 报告语言与表头跟随界面语言；若干加载文案与表头歧义

### 安全

- 删除文件或目录一律经过删除级安全门（含用户数据与同步冲突红线），不依赖调用方自觉
- 系统加密失败时不再明文保存 AI API Key，改为保存失败并提示
- 孤儿清理关联同样走删除级安全门；快捷方式扫描覆盖非 C 盘系统数据目录
- 智能说明回填歧义时跳过，不猜测对应项

### 变更

- 清理报告明细中的跳过原因改为可读说明（用户数据 / 共享组件 / 未关联 / 安全门等）
- 发布正文取自本变更日志的对应版本节；缺节或空节会直接导致发布失败，而不是退回自动生成的提交列表
- 安装包缺失时发布工作流失败，而不是发出没有资产的 Release
- 内部模块整理与测试加固，对外功能接口不变
- 公开仓库不再附带完整文档树；说明以 README、本变更日志与应用内提示为准

## [1.1.0] - 2026-09-20

### Highlights
- **Deep-uninstall UX loop**: software row → detail panel → rich confirm → official uninstaller → auto-scan → classify → you confirm → optional backup → cleanup → report
- **System-item semantics**: startup shows enabled/disabled; services split run state vs start type (stop ≠ disable, with `set_service_running` IPC); tasks show last/next run
- **Orphan trust**: expandable judgment evidence, bulk select safe/review only, page-level scan status, single-channel toast
- **AI demoted to capability**: smart filter collapse + model settings wording (not a page hero)
- **Risk tiers** on cleanup / force-clean / orphan confirms; force-clean analyzes first

### Added
- **Deep-uninstall UX loop**: rich uninstall confirm (official → scan → classify → confirm → optional backup); five-stage progress bar (`identify/official/scan/analyze/report`); report close-out line after full cleanup
- Software list density: publisher DN collapsed via `prettyPublisher`; at most one high-signal chip per row
- Right detail panel trust copy: linked-item empty state + deep-uninstall expectation under primary action
- Right detail panel: linked leftover buckets (program files / config / registry / shortcuts / startup) with size or count; deep-uninstall recommendation card; drill-down filters the leftover table by bucket
- **ManageItem status fields** (optional IPC): `kind` / `running` / `start_type` / `source_label` / `last_run` / `next_run` / `path`
- **Service stop/start IPC** (`set_service_running` + `sc start/stop`) — stop ≠ disable start type; confirm dialogs distinguish the two
- Orphan leftovers: expandable judgment evidence (no-owner / exe / multi-file / config / install-root / mtime age); bulk select safe/review; page-level scan progress; single-channel toast
- Risk-tier labels on cleanup / force-clean / orphan confirm dialogs (`maxRiskOf` / `riskTierLabel`); force-clean analyzes first, then confirms
- Toolbox hierarchy: Everyday / Advanced / System & help; selection-required tools jump to the software list; current-app chip
- Smart-filter collapse on software toolbar (AI demoted from page hero to capability) + example chips
- **AI decision layer**: auto cleanup conclusion after scan (rule-first, AI-labeled when configured); conclusion actions (clean suggested / review / why-keep)
- Copilot on the software list (NL plan → filter / analyze / batch with confirm; offline keyword fallback)
- Report fixed narrative + “next step” lines; auto AI report reading when enabled; first-scan optional hint for clearer explanations
- Uninstall mode actions moved into the `⋯` menu (official / deep / force / analyze)
- Leftover `size_kb` on file/dir items from a bounded directory walk; backend `CleanupItem.bucket` classification (frontend prefers server bucket)
- Portable zip artifact on GitHub Release workflow
- Typed frontend API for history / manage / AI intent; AI + shell state hooks (`useAiPanelState`, `useShellState`); shared `fsutil`
- Size-estimate batch generation (stale results discarded after cancel/begin); residual / list-filter state hooks (`useResidualState`, `useListFilterChrome`)
- Update check resolves NSIS setup asset; footer opens installer download
- Explorer context menu / cleanup verify moved into `sysops`
- Shared panel style tokens (`panelShell`, `sectionTitle`, `detailRow`, …)

### Changed
- Cleanup backup is **opt-in**: confirm dialogs offer an unchecked “create safety backup” option; default cleanup/batch/orphan/force-clean paths no longer force `backup_enabled: true`
- AI copy demoted: 「AI 详细说明/助手」→「模型设置 / 智能解释 / 智能筛选」; chips show model ready/not set; 「智能设置」→「详细说明」 with AI explain off the scan toolbar as primary action
- Startup list shows 已启用/已禁用 + source (registry/folder/store/service), not 「运行中」
- Services show run state + start type; tasks show last/next run when available
- Manage list icons neutral gray; blue reserved for action/status/selection
- Analyze/uninstall toasts use `analyze-flow` channel (no stacked scan/done pair)
- README product positioning documents the deep-uninstall loop; toolbar ⓘ guide describes the full path
- Cleanup gate: PATH danger check expands `%VAR%` and is limited to system-shaped trees; Registry/Path leftovers require app association when an installed app is known; optional `cleanup_source` on full cleanup; `..` segments rejected; association ignores client `reason`; Monitor/Orphan cleanup always skips the official uninstaller
- Shared leftovers: Common Files **roots** and `Microsoft Shared` stay hard-blocked; vendor subpaths may be cleaned only when the **vendor directory segment** matches install/name/publisher; the confirm dialog warns when Common Files paths are selected
- Restore point decoupled from backup; backup abort surfaces as failure
- Dry-run counts align with full delete (missing File/Dir and absent PATH segments skipped); PATH probe uses the same registry source as scrub; history records delayed deletes
- Manage: rejects `Microsoft*` service writes; `FOLDER::` only accepts known Startup folders; `MANAGE_LOCK` serializes manage mutations; PATH restore without scope evidence defaults to User only; scheduled-task native delete uses the full TaskCache path
- Cleanup report counts reboot-delayed deletes separately from `deleted`; `sc`/`schtasks` native results recorded
- AppData / WebView name-match folders default to **suspected/medium** (not auto-selected)
- MSI uninstall command only when `msiexec` is present or the string is a bare `{GUID}`
- Scan leftover risk-filter chips show real confirm/keep counts (was hardcoded 0)
- Leftover list virtualized for large scan results; size estimates flush in batches
- Batch cleanup still runs the official uninstaller when no default-selectable leftovers exist
- Version narrative unified at **1.1.0** (skip separate 1.0.1 release)
- **Architecture**: scanner split into `scanner/{mod,fs_scans,reg_scans}`; `policy.rs` façade so dry-run/full share `gate_cleanup_item`; install monitor tighter roots with cache/temp/log diffs demoted; `SoftwarePage` `React.memo` + controller hook; `CategoryId` / `formatSize` single-sourced; App no longer re-exports types
- **Frontend structure**: domain reducers (`src/hooks/reducers/`) for shell / residual / aiPanel / scanUi / listFilter / appCore; MorePage business hooks; scan UI chrome in `useScanUiState`; i18n split `src/i18n/{zh,en,index}.ts`; residual/AI/Shell hooks expose an `actions` API
- **Shell setters** apply real functional updaters (not toggle-on-fn)
- Route-level code-split for Software / Manage / More / Orphan pages; ScanActions `busy` uses a state expression instead of a ref during render
- `Safety IPC`: registry path gates emit `safety:protected::*` codes
- **CI**: npm + rustc dependency caches; `clippy --all-targets`; ESLint flat config + `npm run lint`; `typecheck:tests`; `scripts/check-commands.ps1`; frontend `dist` smoke; release portable zip smoke; version consistency script; Release attaches CHANGELOG body
- **Docs**: internal planning-doc link paths fixed; acceptance notes historical v0.1.0 + current baseline **1.1.0**; docs index aligned to package version; CHANGELOG Unreleased de-duplicated; ARCHITECTURE restore order + env prefixes + dynamic service count documented
- `.gitignore`: track the user-facing docs; keep working notes and release-planning drafts private; release notes no longer dump the whole CHANGELOG body
- Portable zip unified via `pwsh` + `scripts/package-portable.ps1`

### Fixed
- Cleanup protection: server-side checks preserve user data, shared components and protected folders even when a request supplies incorrect flags
- PATH cleanup preserves system entries such as Windows, System32 and PowerShell
- **PATH leftover Safety Vault**: production backup snapshots PATH segments to `path.json` (no tree copy); restore merges missing segments only
- **Manage IPC safety**: PackagedStartup writes restricted to StartupApproved keys; Run locations must map to known Run/RunOnce keys; `set_task_enabled` rejects `\Microsoft\Windows\*` system tasks
- Expanded critical service names for manage disable/list; `is_safe_fs` protected prefixes include SystemRoot / ProgramData / ProgramFiles / SystemDrive
- **Ignore rules enforced in backend**; registry value restore prefers `value.reg`
- Association checks skip unrelated leftovers; name matching requires enough evidence and rejects generic names
- Orphan flow uses the safety gate only (no fake slug gate)
- Cleanup previews apply the same user-data, shared-component, ignore-list and association checks as deletion
- `setMulti` / `setSelectedPaths` functional React updaters resolved inside the reducer (no lost concurrent updaters; residual checkbox & batch multi)
- Uninstall / official-uninstall entry points reject re-entry while busy; empty leftover selection cannot confirm cleanup
- Backup only processes items that pass the cleanup gate; skipped items failing backup do not abort; `path_map.json` write failure counts as a backup fail and aborts cleanup; value.reg export failure aborts backup with `backup:value_reg`
- PATH restore takes `PATH_LOCK`; `reg.exe` writes translate `HKLM64`/`HKLM32` hive aliases; PATH read failure no longer falls back to the process `PATH` (structured `path:io` IPC)
- SVC enable/disable uses manual (3)/disabled (4) + `MANAGE_LOCK`; full delete counts already-missing File/Dir as skipped (not deleted)
- Executor skips `shared` leftovers and ignore-list paths at delete time; cleanup and PATH scrub share a process mutex; appCore reducer single-sourced
- Empty leftover paths filtered before the full cleanup abort check
- **RemovaError**: backup/restore/PATH critical paths emit `backup:*` / `restore:*` / `path:io` codes; AI commands surface model/network failures as `ai:*` instead of silent empty results; frontend `formatError` maps them
- FN-04 rescan wired after cleanup; single ErrorBanner on the software page; More-page install-monitor diff panel always visible when `monitorDiff` exists
- Orphan page: default-safe selection + error banner hook; shell/AI hooks return stable identities for memo
- Stable software-list callbacks so `AppRow.memo` is not defeated; MorePage badges use i18n (`badgeNew` / `badgeRunning`)
- Dead frontend components removed (`RelationGraph` / `RelationOverview`); `ManageItem` type lives in `types.ts`; `NlIntent` single type
- Open location: validate path exists, select files in Explorer, fall back if System32 explorer fails, and map missing-path errors to actionable text
- Executor never deletes `user_data` paths (hard skip + report)
- History CSV escapes quotes/commas in all string fields
- `list_backup_sessions` runs on the blocking pool (no longer sync on the command thread); `backup_item` delegates to production `backup_item_with_map`; cleanup backup stage extracted to `try_backup_phase`; shared `fsutil::fnv1a64` with magic numbers centralized in `constants.rs`
- Testing Library + jsdom coverage: confirm store, CleanupConclusion, hooks actions; `batchEngine` covers ok / no-leftover uninstall / failed / cancel / invoke error
- ESLint exhaustive-deps cleaned; lint exits 0
- Public docs index no longer links private working-note paths; USER-GUIDE AI entry is 「详细说明」 with conclusion card + Copilot documented
- ARCHITECTURE command table synced with `generate_handler!` (removed ghost `restore_latest_backup`; added AI/verify/backup session commands); ARCHITECTURE / USER-GUIDE aligned with shipped features

### Security
- Cleanup executor enforces the user-data red line even if a path slips past scan-time marking
- AI key stored via DPAPI and never returned to the frontend; cloud payloads pass through `sanitize_path`

## [1.0.0] - 2026-09-16

First public release.

### Added
- Installed software list with search, size / recent filters, and a fixed detail panel
- Deep uninstall flow: official uninstaller, leftover scan, dry-run, cleanup with Safety Vault backups
- Leftover evidence: confidence, risk, relation overview, and user-data protection (Documents / Downloads / sync never auto-selected)
- PATH cleanup with exact segment matching and environment broadcast
- Orphan leftover page and install monitor for before/after diffs
- Startup / services / scheduled tasks management
- History timeline, backup restore, ignore rules, and optional AI advisory (off by default)
- Custom title bar (no OS chrome) with in-app minimize / maximize / close
- System tray icon; close can minimize to tray or quit (More → 关闭窗口时)
- NSIS installer and portable zip for Windows x64

### Security
- AI API key stored with Windows DPAPI at rest
- Shared-runtime selections require explicit confirmation
- System paths and critical services are blocked from cleanup

<!-- R-R6-08: link definitions live at the bottom of the file (Keep a Changelog). -->
[1.1.1]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.1.1
[1.1.0]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.1.0
[1.0.0]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.0.0
