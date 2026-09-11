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
    dryRun: "演练清理",
    cleanup: "备份并清理",
    closePreview: "关闭预览",
    useOfficial: "调用官方卸载器",
    batch: "批量清理",
    colName: "名称",
    colVersion: "版本",
    colPublisher: "发布者",
    colSource: "来源",
    colLocation: "安装路径",
    confirmed: "★★★ 确定",
    suspected: "★★ 疑似",
    low: "★ 低",
    admin: "管理员",
    nonAdmin: "非管理员",
    disk: "磁盘",
    guided:
      "快速上手：选中软件 → 深度分析 → 演练清理 → 确认后备份并清理。可多选后「批量清理」。",
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
    dryRun: "Dry-run cleanup",
    cleanup: "Backup & cleanup",
    closePreview: "Close preview",
    useOfficial: "Run official uninstaller",
    batch: "Batch cleanup",
    colName: "Name",
    colVersion: "Version",
    colPublisher: "Publisher",
    colSource: "Source",
    colLocation: "Install path",
    confirmed: "★★★ Confirmed",
    suspected: "★★ Suspected",
    low: "★ Low",
    admin: "Admin",
    nonAdmin: "Not admin",
    disk: "Disk",
    guided:
      "Quick start: select an app → deep analyze → dry-run → backup & cleanup. Multi-select for batch.",
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

export function t(): (typeof dict)["zh"] {
  return dict[lang];
}
