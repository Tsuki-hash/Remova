# Security policy / 安全策略

## Supported versions / 支持的版本

| Version | Supported |
| --- | --- |
| 1.4.x | ✅ |
| < 1.4 | ❌ |

## Reporting a vulnerability / 报告漏洞

**EN**: Please report security issues privately via GitHub's **Private vulnerability reporting** on this repository (Security tab → Report a vulnerability). Do not open a public issue for anything you believe is exploitable.

Remova is a deep-uninstall tool that deletes files, edits the registry, and can run elevated. Findings of special interest:

- Anything that widens what the deletion / restore / migration pipelines may touch beyond their server-side admission checks (path traversal, junction or symlink redirection, IPC parameter abuse);
- Escalation from the non-admin UI into privileged operations;
- Backup integrity (DPAPI/HMAC sealing) and updater signature handling;
- Supply chain: build workflow downloads, SBOM accuracy, dependency advisories.

Please include a reproduction (steps, paths, build) and your assessment of impact. You will get an initial response within 7 days; we will keep you updated while a fix is prepared and credit you in the release notes if you wish.

**中文**：请通过 GitHub 仓库的 **Private vulnerability reporting**（Security 页 → Report a vulnerability）私下报告安全问题；不要为可利用的问题开公开 issue。

Remova 是会删除文件、修改注册表、并可能以管理员权限运行的深度卸载工具。以下方向的问题尤其受关注：

- 删除 / 还原 / 目录迁移管线越过其服务端准入检查的任何途径（路径穿越、junction/符号链接重定向、IPC 参数滥用）；
- 从非管理员界面到特权操作的提权；
- 备份完整性（DPAPI/HMAC 密封）与更新器签名处理；
- 供应链：构建工作流下载、SBOM 准确性、依赖公告。

请附复现步骤（步骤、路径、构建）与影响评估。我们会在 7 天内首次回应，修复期间保持同步；如你愿意，发布说明中会署名致谢。
