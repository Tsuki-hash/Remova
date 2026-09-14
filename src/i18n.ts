/** Simple zh/en dictionary for Remova-next UI. */

export type Lang = "zh" | "en";

const dict = {
  zh: {
    title: "Remova",
    subtitle: "Deep Uninstall · Tauri + React + Rust",
    search: "搜索名称 / 发布者 / 安装路径…",
    history: "历史",
    exportCsv: "导出 CSV",
    restore: "还原最近备份",
    analyze: "深度分析",
    analyzing: "分析中…",
    dryRun: "仅预览",
    cleanup: "清理选中项",
    closePreview: "关闭预览",
    useOfficial: "调用官方卸载器",
    batch: "批量清理",
    colName: "名称",
    colVersion: "版本",
    colPublisher: "发布者",
    colSource: "来源",
    colLocation: "安装路径",
    colSize: "占用",
    colInstallDate: "安装日期",
    selectRowHint: "请先点击列表中的软件行",
    confirmed: "★★★ 确定",
    suspected: "★★ 疑似",
    low: "★ 低",
    admin: "管理员",
    nonAdmin: "非管理员",
    disk: "磁盘",
    guided:
      "选中软件 → 深度分析 → 勾选要清理的项 →「清理选中项」（会先备份）。不确定时可先「仅预览」。",
    closeGuide: "知道了",
    versionNew: "发现新版本",
    themeToggle: "深色/浅色",
    langToggle: "English",
    batchConfirm: (n: number) =>
      `将对 ${n} 个已选软件依次分析并清理「确定」项（跳过高风险）。继续？`,
    cleanupConfirm: (n: number, official: boolean) =>
      `将备份并清理 ${n} 项${official ? "并调用官方卸载器" : ""}。确认？`,
    restoreConfirm: "将从最近备份还原文件与注册表。继续？",
    noHistory: "暂无记录",
    historyTitle: "清理历史",
    errorDismiss: "知道了",
    adminHint: "当前不是管理员，清理系统软件可能失败。点击以管理员身份重启。",
    adminAlready: "已是管理员。",
    errElevateDenied:
      "提权失败：访问被拒绝。请在 UAC 弹窗中选择「是」；若仍失败，请右键 Remova 选择「以管理员身份运行」。",
    errElevateCancelled: "已取消管理员提权，当前仍以普通权限运行。",
    errElevateNotFound: "找不到 Remova 可执行文件，无法提权重启。请重新安装或从安装目录启动。",
    errElevateFailed: (code: number) =>
      `提权失败（代码 ${code}）。可尝试右键以管理员身份运行 Remova。`,
    errAnalyzeFailed: (detail: string) => `深度分析失败：${detail}`,
    errCleanupFailed: (detail: string) => `清理失败：${detail}`,
    errInvokeFailed: (detail: string) => `操作失败：${detail}`,
    stopEstimate: "停止估算",
    estimatingSizes: "估算占用中…",
    batchCancel: "取消批量",
    batchCancelHint: "将在当前应用完成后停止",
    batchDone: "批量完成",
    batchCancelled: "已取消批量",
    batchSummary: "批量结果",
    batchOk: "成功",
    batchFailed: "失败",
    batchSkipped: "无残留跳过",
    batchRetryFailed: "重试失败项",
    batchDismiss: "关闭",
    restoreSessions: "备份会话",
    restoreNoSessions: "暂无备份会话",
    restoreSelect: "选择要还原的会话",
    restoreRun: "确认还原",
    restoreClose: "关闭",
    restoreResult: "还原结果",
    restoreLoading: "加载会话中…",
    manage: "管理",
    manageStartup: "启动项",
    manageServices: "服务",
    manageTasks: "计划任务",
    manageEnable: "启用",
    manageDisable: "禁用",
    manageReload: "刷新",
    manageClose: "关闭管理",
    forceClean: "强制清理残留",
    forceCleanHint: "将跳过官方卸载器，仅清理已确认的残留项（会先备份）。",
    shellMenu: "注册右键菜单",
    shellMenuOn: "已注册右键菜单",
    shellUnregister: "取消右键菜单",
    dropHint: "可将 exe/安装目录拖入窗口以分析",
  },
  en: {
    title: "Remova",
    subtitle: "Deep Uninstall · Tauri + React + Rust",
    search: "Search name / publisher / path…",
    history: "History",
    exportCsv: "Export CSV",
    restore: "Restore latest backup",
    analyze: "Deep analyze",
    analyzing: "Analyzing…",
    dryRun: "Preview only",
    cleanup: "Clean selected",
    closePreview: "Close preview",
    useOfficial: "Run official uninstaller",
    batch: "Batch cleanup",
    colName: "Name",
    colVersion: "Version",
    colPublisher: "Publisher",
    colSource: "Source",
    colLocation: "Install path",
    colSize: "Size",
    colInstallDate: "Installed",
    selectRowHint: "Click a software row first",
    confirmed: "★★★ Confirmed",
    suspected: "★★ Suspected",
    low: "★ Low",
    admin: "Admin",
    nonAdmin: "Not admin",
    disk: "Disk",
    guided:
      "Select an app → deep analyze → check items → Clean selected (backs up first). Use Preview only if unsure.",
    closeGuide: "Got it",
    versionNew: "New version available",
    themeToggle: "Dark/Light",
    langToggle: "中文",
    batchConfirm: (n: number) =>
      `Analyze ${n} selected apps and clean CONFIRMED items (high risk skipped). Continue?`,
    cleanupConfirm: (n: number, official: boolean) =>
      `Backup and clean ${n} items${official ? " with official uninstaller" : ""}. Confirm?`,
    restoreConfirm: "Restore files and registry from latest backup?",
    noHistory: "No records",
    historyTitle: "Cleanup history",
    errorDismiss: "Dismiss",
    adminHint: "Not running as administrator. Cleaning system software may fail. Click to restart as admin.",
    adminAlready: "Running as administrator.",
    errElevateDenied:
      "Elevation failed: access denied. Click Yes on the UAC prompt; if it still fails, right-click Remova and run as administrator.",
    errElevateCancelled: "Admin elevation cancelled. Still running with standard privileges.",
    errElevateNotFound: "Remova executable not found; cannot restart elevated. Reinstall or launch from the install folder.",
    errElevateFailed: (code: number) =>
      `Elevation failed (code ${code}). Try running Remova as administrator.`,
    errAnalyzeFailed: (detail: string) => `Deep analyze failed: ${detail}`,
    errCleanupFailed: (detail: string) => `Cleanup failed: ${detail}`,
    errInvokeFailed: (detail: string) => `Operation failed: ${detail}`,
    stopEstimate: "Stop estimating",
    estimatingSizes: "Estimating sizes…",
    batchCancel: "Cancel batch",
    batchCancelHint: "Stops after the current app finishes",
    batchDone: "Batch finished",
    batchCancelled: "Batch cancelled",
    batchSummary: "Batch results",
    batchOk: "OK",
    batchFailed: "Failed",
    batchSkipped: "No leftovers",
    batchRetryFailed: "Retry failed",
    batchDismiss: "Dismiss",
    restoreSessions: "Backup sessions",
    restoreNoSessions: "No backup sessions",
    restoreSelect: "Select a session to restore",
    restoreRun: "Restore",
    restoreClose: "Close",
    restoreResult: "Restore result",
    restoreLoading: "Loading sessions…",
    manage: "Manage",
    manageStartup: "Startup",
    manageServices: "Services",
    manageTasks: "Tasks",
    manageEnable: "Enable",
    manageDisable: "Disable",
    manageReload: "Reload",
    manageClose: "Close manage",
    forceClean: "Force clean leftovers",
    forceCleanHint: "Skips the official uninstaller and cleans confirmed leftovers only (backup first).",
    shellMenu: "Register context menu",
    shellMenuOn: "Context menu registered",
    shellUnregister: "Unregister context menu",
    dropHint: "Drop an exe or install folder to analyze",
  },
} as const;

let lang: Lang = "zh";

export function loadLang() {
  const v = localStorage.getItem("remova_lang");
  if (v === "en" || v === "zh") lang = v;
  return lang;
}

export function setLang(l: Lang) {
  lang = l;
  localStorage.setItem("remova_lang", l);
}

export function currentLang() {
  return lang;
}

export type Strings = Omit<
  (typeof dict)["zh"],
  "batchConfirm" | "cleanupConfirm" | "errElevateFailed" | "errAnalyzeFailed" | "errCleanupFailed" | "errInvokeFailed"
> & {
  batchConfirm: (n: number) => string;
  cleanupConfirm: (n: number, official: boolean) => string;
  errElevateFailed: (code: number) => string;
  errAnalyzeFailed: (detail: string) => string;
  errCleanupFailed: (detail: string) => string;
  errInvokeFailed: (detail: string) => string;
};

export function formatSize(kb: number): string {
  if (!kb || kb <= 0) return "—";
  if (kb < 1024) return `${kb} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}

export function t(): Strings {
  return dict[lang] as Strings;
}
