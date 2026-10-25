# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.4.1]

### 新增

- 新增「系统痕迹与便携应用」只读报告：列出 ConsentStore 隐私授权中指向已不存在程序的孤儿条目（结构已通过评估，删除能力按计划暂不开放），以及防火墙规则交叉引用出的便携应用候选（存在但不在任何已安装记录安装位置内）；Tile、RADAR、JumpList 未通过评估闸，本周期不落地并在界面如实说明。
- **目录迁移（junction）**：把用户数据或程序目录整体迁往其他盘，全程只移动不删除——「计划」先行（准入校验：拒绝系统树、MSIX、Squirrel 更新器、硬链接树、嵌套路径等）、「复制」逐文件双向校验且可取消重跑、「切换」在原位创建 junction 并自动验证写入（失败自动改回原名）、「收尾」逐文件核对后才在确认下删除 `<源>.bak` 备份；切换后随时可一键回滚。每一步记入迁移台账。
- 新增「全局 CLI 包」清单（npm / pnpm / yarn 只读）：npm 全局包支持在确认后删除——严格准入（仅确认的全局 node_modules 下的普通包目录，拒绝符号链接/junction/路径穿越/裸 scope）、删除前整包与命令 shim 导出到安全备份、备份失败不删、服务端新扫描复核；pnpm 与 yarn 保持只读（链接布局评估后再开放）。
- 「C 盘空间体检」面板新增可再生缓存清理：仅限固定布局（Windows 更新下载缓存、系统/用户临时目录、崩溃转储、WER 报告、着色器缓存），默认不勾选，确认时明示不创建备份（内容会自动重建）；只删内容、目录本身保留，被占用文件如实跳过并逐项记账释放量；系统组件服务（servicing）进行中时 CbsTemp 暂不可清理。

### 改进

- 主题新增「跟随系统」：默认跟随 Windows 深浅色设置并实时切换，也可显式选择浅色或深色；此前保存过主题偏好的用户保持原选择不变。
- 首次运行展示三条安全原则（先检查再执行、默认可恢复、高风险不预选），不阻塞列表，可随时在「更多 → 帮助 → 安全说明」重新查看。
- 扫描进度显示已用时间；取消或失败后耗时仍如实展示，不做完成时间预估。
- 「更多」页工具箱按「常用 / 安装健康 / 空间与缓存 / 系统痕迹 / 系统与帮助」重新分组；工具超过八个时提供过滤框，「最近使用」会置顶最近打开过的三个工具。
- 新增「C 盘空间体检」只读面板：系统盘容量、目录链接（junction）清册（含目标不可达的死链标记与不可读根说明）、分流视图（各目录按「可再生清理 / 仅 DISM / 迁移候选 / 不动 / 只展示」标注并附命中依据与体积）、以及需要管理员权限的 WinSxS 组件存储 DISM 只读分析；本面板不执行任何删除或迁移。

## [1.4.0] - 2026-10-24

本次发布汇总自 v1.3.1 以来的安全、正确性与可用性改进，并新增深度卸载、安装包与缓存扫描能力。

> 发布说明：1.3.1 未配置应用内更新公钥，升级到 1.4.0 请从 Release 页面手动下载；若安装包未提供 Authenticode 签名，请使用同页 `SHA256SUMS.txt` 校验。

### 新增

**深度卸载**

- **损坏的安装记录**：安装目录已丢失但 Uninstall 记录仍在的 MSI 应用会在「更多」页单独列出，可直接重新运行官方卸载器并自动重扫；只读扫描，不自动删除任何记录。
- **服务绑定保护**：候选目录若承载已注册 Windows 服务的 ImagePath，会标记为保留项、默认不勾选，删除时直接拒绝，避免删掉仍在使用中的服务数据目录。
- **活跃孤儿保护**：孤儿目录近 3 天仍有写入时提升为高风险并注明「近期仍在写入」，不再作为普通残留展示。
- **空壳目录裁剪**：删除应用目录后清理名称匹配产品/厂商且确为空的祖先目录（如 `Program Files\Vendor`），不越过已知根、不处理 reparse 点。
- **损坏记录清理**：重新运行卸载器仍无效时，可二次确认后删除损坏记录；相关 Uninstall 键与 `Installer\Products` 孤儿键会先导出到安全备份，再按精确形状删除。
- **孤儿防火墙规则**：新增只读扫描与「孤儿网络规则」面板，列出程序文件已不存在的规则；删除需确认、服务端重新校验并先备份注册表值。
- **主目录点目录与 MuiCache**：补扫 `%USERPROFILE%\.<产品>` 及共享点根下的匹配子目录（共享根本身永不作为目标），并列出可执行文件已不存在的本应用 MuiCache 显示名缓存；MuiCache 只允许删除 `FriendlyAppName` 值。
- **锁占用诊断**：失败项可通过 Restart Manager 只读查询占用进程名、PID 与映像路径；确认后可结束映像位于该应用安装目录内的进程并重试，服务端会重新校验实时映像，拒绝自身、目录外与非 exe 进程。
- **只读属性处理**：删除树遇到 READONLY/HIDDEN/SYSTEM 属性时先清除属性再重试一次，不跟随 reparse 点。
- **清理报告总账与复核**：清理报告新增「实删 / 保留 / 待重启」体积总账，并提供「重新扫描该应用」（不预选任何新发现）与「深度核查注册表」（有界只读、仅报告不删除）；同名多安装记录会显示「多个安装」标记，删除语义保持逐项独立。

**安装包与缓存扫描**

- 识别 `%LOCALAPPDATA%` 下以 `<产品名>-updater` 结尾的一级更新器目录，并修正 pnpm、Gradle 等此前从未生效或写错的缓存路径。
- 下载目录的安装包识别不再依赖文件名：除安装包扩展名与中英文安装名称外，还会校验 `.exe` 的 PE 头；无法确认用途的条目标为「可能是安装包」，默认不勾选。
- 下载目录位置改用系统「下载」Known Folder；归档类文件在名称含安装特征时也会列出并默认不勾选；小于 8 MB 的专用安装包不再被体积门槛跳过。
- 尊重 `CARGO_HOME`、`GOPATH`、`PIP_CACHE_DIR`、`NUGET_PACKAGES`、`GRADLE_USER_HOME`、`UV_CACHE_DIR`、`PNPM_HOME`、npm 缓存配置与 `XDG_CACHE_HOME` 等重定位变量。
- 编辑器缓存按目录结构识别（同时存在 `Cache` 与 `CachedData` 即视为该系列），覆盖 VS Code、Insiders、VSCodium、Cursor、Windsurf 等分支。
- 浏览器缓存覆盖全部配置文件与 `Cache` / `Code Cache` / `GPUCache`，产品覆盖 Chrome、Edge、Brave、Vivaldi、Chromium 与 Opera / Opera GX。
- 多个工具共用的缓存根改为逐项列出（如 `~/.cache`、`%LOCALAPPDATA%\JetBrains`），共享根本身不会作为清理目标；JetBrains 仅列出 `caches`、`compile-server`、`log`、`tmp`。
- 补充 uv、Bun、Go module、Maven、NuGet packages 与 VS Code 缓存目录。
- 扫描先完整枚举再统一排序截断，结果不再依赖文件系统顺序；超出上限的条目会如实显示「另有 N 项未显示」。

### 改进

**性能**

- 服务快照在一次批量分析内复用（30 秒），取消的扫描不缓存不完整列表，破坏性操作后立即失效。
- CLSID 默认值与注册表子键增加扫描级缓存，批量分析不再为每个应用重复全量枚举；缓存触顶时淘汰最旧项，并记录「无值/非字符串」结果。
- 还原按父目录复用钉住句柄；磁盘雷达预算在各扫描根间公平分配；体积估算按行分配子配额并支持并行 worker。
- TEMP 与快捷方式条目预计算匹配字段；MuiCache、Known Folder 子项与服务 image key 按批次缓存。
- 孤儿匹配预计算安装位置与名称词元；安装包嵌套判断与忽略列表读取去掉重复扫描。
- 前端列表统计改为记忆化，体积估算进度合并发布，软件排序复用推荐分，虚拟滚动减少长列表渲染开销。
- 目录枚举减少重复 stat；后台遍历显式限流，前台请求不会被批量扫描堵住。

**可用性**

- 扫描显示阶段与已发现数量，支持只读扫描取消；取消后明确标注结果不完整，迟到结果不能覆盖新任务。
- 清理结果分开展示已删除、失败、跳过和等待重启；失败项提供路径、原因、定位与重试。
- 清理确认实时展示是否创建备份及后果；无有效备份不承诺可恢复，备份失败继续中止清理。
- 安全仓还原预览标注目标状态（可还原 / 已存在 / 备份缺失 / 受保护不可用），确认前自动重新校验。
- 列表支持全选/取消当前筛选结果、保留隐藏已选项、类型与风险叠加筛选，以及「只看已选」。
- 键盘可访问性完善：主列表行 Enter/Space 只处理行自身，菜单支持方向键与 Home/End，长按确认提供键盘路径，补齐可访问名称与 `aria-sort`。
- 中英文文案与证据标签统一本地化；扫描降级原因使用稳定代码，界面不再出现英文内部错误串。
- 删除损坏安装记录在需要管理员权限时说明原因（MSI 安装数据位于机器级注册表）并提供一键以管理员身份重启，不再透传英文错误。
- 孤儿网络规则面板支持一键全选/取消全选后批量删除；删除防火墙规则需要管理员权限时同样提供一键以管理员身份重启。
- 发布产物附带 CycloneDX SBOM（Cargo.lock 与生产 npm 依赖，确定性生成并随 Release 发布）；npm 依赖审计全绿（含开发链）。
- 长路径可复制，列表滚动与窄窗口布局优化；空态区分「没有可清理项」与「以下位置无法读取」。
- 清理报告默认收起明细，保留卸载结果与复扫计数；详情抽屉、判定依据面板与工具面板可自动滚入视野。

### 修复

- 修复批量分析在拒绝/失败时泄漏扫描任务槽的问题，避免累计拒绝后所有扫描返回 `scan:busy`；批量扫描不再绕过 8 任务上限与取消机制。
- 清理与预览开始前重置取消标志，不再继承上一次 dry-run 或批次的取消状态。
- 专项清理（孤儿/监控/安装包/工具缓存）只接受服务端授权清单中的条目，客户端改写 `kind` 或 `service_bound` 会被拒绝。
- 提权状态下拒绝 HKCU 应用的分析与卸载；跨 hive 同名条目不再合并，机器级身份不从用户可写 hive 采纳。
- 服务绑定保护覆盖孤儿、安装包与专项缓存扫描；ImagePath 先展开环境变量并正确解析含空格路径。
- 去重改为按「类型 + 路径身份」，安装根与同名 PATH 项同时保留；清理、报告与备份不再重复处理同一路径。
- TEMP 同时扫描 `LocalAppData\Temp` 与重定向 `TEMP`；某一根不可读时保留可读根结果并标记「TEMP 不完整」，30 秒后重试。
- 修复工具缓存根在 UNC、设备路径、相对路径与父目录穿越上的探测风险；重复根只报告一次。
- 修复服务快照、子键缓存、MuiCache 与文件列表在清理/官方卸载后的失效范围，未触及注册表时不再全量清空。
- 修复 MuiCache 后缀大小写、防火墙 `App=` 引号、孤儿规则系统资源项与 MuiCache 枚举上限。
- 修复锁终止授权：结束进程前由后端确认该路径属于最近一次清理的授权根；孤儿清理的「结束进程并重试」现在会真正发起请求。
- 修复 MSI 重启语义：3010 按成功处理并继续清理，1641 视为卸载成功但跳过残留删除，并在重启前阻断后续破坏性入口。
- 修复损坏 MSI 记录误报：只接受真正的 `msiexec /x` 命令，缺失目录必须位于本地固定盘；`InstallLocation` 先展开变量并去除引号。
- 修复同名安装错配、残留清理中止后错误启用官方卸载器、报告重扫状态误标「卸载已完成」等问题。
- 修复清理总账重复计体积，新增「待重启」体积，实删/保留/待重启口径一致。
- 修复扫描根不可读时静默返回空结果、扫描结果被上限静默截断，以及体积估算阶段进度停滞。
- 修复安装监视器漏扫 `%APPDATA%` / `%ProgramData%`、把 Remova 自己的快照与封印密钥当成残留的问题。
- 修复统一路径规范化中的尾随点/空格、UNC/设备路径与相对路径处理；授权清单、PATH、孤儿与安全红线共用同一实现。
- 修复官方卸载拒绝相对路径镜像、卸载命令解析失败仍继续删除、清理结果在 `failed = 0` 但实际未删除时误报成功。
- 修复备份文件名按 UTF-16 单元预算截断，超长与 emoji 文件名不再让整次清理中止；超过 260 字符的路径使用扩展长度路径。
- 修复防交换守卫在名称碰撞与盘根目录上的写入，标记只落在本次操作的目标目录。
- 修复历史文件写入与重写：拒绝硬链接与父链 reparse，删除、压缩、清空统一钉住父目录并重设私有 ACL。
- 修复注册表读取失败被当成「值不存在」，导致导出的 `.reg` 静默漏值；大于 1 MB 的值明确报错而不是无备份。
- 修复卸载器已删除的注册表项被记为失败，现与文件/目录「路径不存在」口径一致，记为「已跳过」。
- 修复安装日期格式解析、清理历史仪表、列表滚动死区、复选框重复计数等界面问题。
- 修复早期批次的安装包与缓存扫描：更新器目录、pnpm/Gradle 路径、上限顺序依赖与缓存类别显示。
- 修复清理流程备份后被拒条目在删除阶段放行的问题：备份与删除只判定一次门禁，删除级校验仍逐项执行。
- 修复还原流程在父目录钉扎不可用时中止断言的问题：改为跳过该目标并记录原因，与其余还原失败路径同为 fail-closed。

### 安全

- 安装监视器整组排除 `%ProgramData%\RemovaSealKeys`、`%ProgramData%\Remova` 与 `%LOCALAPPDATA%\Remova`，避免删除封印密钥导致备份永久无法还原。
- `%APPDATA%` / `%ProgramData%` 下的新增项降为「疑似 / 中风险」，永不预勾选。
- 共享运行时令牌按路径段锚定匹配，避免 `Community`、`Opportunity` 等普通名称被误判为共享组件；Unity、Python、Visual C++、DirectX、PhysX、JRE/JDK、WebView2 仍按共享处理。
- 提权进程在 MuiCache、空闲雷达、图标/AI 配置根与 Known Folder 根上先校验本地固定盘，UNC/重定向路径直接跳过或失败关闭。
- AI 应用名与发布者在发送前脱敏；自定义端点仅允许 HTTPS（HTTP 仅限本机回环），拒绝 userinfo 与伪回环；AI 文本按界面语言生成与缓存。
- 备份目录与会话设置私有 DACL（SYSTEM/Administrators + 所有者），历史文件写入后同样加固；只有新建会话严格加固，历史/跨账户会话尽力而为。
- 注册表保护先规范化显式 `WOW6432Node`，不能绕过 Windows/Cryptography 保护前缀；删除门禁拒绝空服务名与裸厂商根。
- 还原写回先钉住目标父目录、按句柄解析真实路径并重跑门禁；叶子为 reparse 点则拒绝写穿；PATH 还原与删除共用门禁。
- `.reg` 还原识别值删除并拒绝；PATH 按原始字节与类型回写，`REG_EXPAND_SZ` 不被展开固化。
- 整键导出、`value.reg` 与还原暂存统一写 UTF-16LE + BOM，`reg import` 不再把中文/日文等非 ASCII 键值按 ANSI 解释成乱码。
- 导出的清理历史 CSV 对公式起始字符做中和，防电子表格公式注入；图标缓存校验 PNG 魔数并原子替换。
- 还原点创建失败返回真实 Win32 错误码，用户可在「更多」中关闭清理前自动创建还原点。

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
- Rescan wired after cleanup; single ErrorBanner on the software page; More-page install-monitor diff panel always visible when `monitorDiff` exists
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

<!-- Link definitions live at the bottom of the file (Keep a Changelog). -->
[1.1.1]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.1.1
[1.1.0]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.1.0
[1.0.0]: https://github.com/Tsuki-hash/Remova/releases/tag/v1.0.0
