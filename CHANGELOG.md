# Changelog

All notable changes to Remova will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.3.1] - Unreleased

### 开发中

- 开始 1.3.1 开发，统一应用版本；尚未发布。
- 软件关联扫描显示阶段和已发现数量，支持只读扫描取消；取消结果标为不完整，重新分析清空旧勾选，迟到结果不会覆盖新任务。
- 清理结果独立显示等待重启数量，失败、跳过、延迟删除和中止结果不再显示全部清理完成。

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
