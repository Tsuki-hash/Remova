/** Thin typed wrappers around Tauri invoke — single frontend data layer. */
import { invoke } from "@tauri-apps/api/core";
import type {
  AiConfigView,
  AiExplainInput,
  AiExplainOutput,
  CleanupItem,
  CleanupReport,
  FullCleanupReport,
  HistoryEntry,
  IgnoreSuggestion,
  InstalledApp,
  ManageItem,
  NlIntent,
  OfficialUninstallResult,
  ScanResult,
} from "../types";

export type FullCleanupOptions = {
  dry_run: boolean;
  skip_official_uninstall: boolean;
  backup_enabled: boolean;
  restore_point?: boolean;
  /** Gate source: uninstall | orphan | monitor | copilot */
  cleanup_source?: "uninstall" | "orphan" | "monitor" | "copilot";
};

export type IgnoreLists = { publishers: string[]; names: string[]; paths?: string[] };
export type DiskUsage = { free_gb: number; total_gb: number; drive?: string };
export type MonitorDiff = { added_files: string[]; added_reg_values: string[] };
export type VerifyRow = { path: string; kind: string; still_there: boolean };
export type BackupSession = { name: string; size_kb: number; created_at: string };

export const api = {
  listApps: () => invoke<InstalledApp[]>("list_installed_apps"),
  analyze: (app: InstalledApp) => invoke<ScanResult>("analyze_associations", { app }),
  dryRun: (
    app: InstalledApp,
    items: CleanupItem[],
    options?: { cleanup_source?: "uninstall" | "orphan" | "monitor" | "copilot" },
  ) => invoke<CleanupReport>("run_cleanup_dry_run", { app, items, cleanupSource: options?.cleanup_source }),
  fullCleanup: (app: InstalledApp, items: CleanupItem[], options: FullCleanupOptions) =>
    invoke<FullCleanupReport>("run_full_cleanup", { app, items, options }),
  officialUninstall: (app: InstalledApp) =>
    invoke<OfficialUninstallResult>("run_official_uninstall", { app }),
  verifyLeftovers: (items: CleanupItem[]) =>
    invoke<VerifyRow[]>("verify_cleanup_leftovers", { items }),
  orphanScan: () => invoke<CleanupItem[]>("scan_orphan_leftovers"),
  isElevated: () => invoke<boolean>("is_elevated"),
  elevateRestart: () => invoke("elevate_restart"),
  openPath: (path: string) => invoke("open_path_in_explorer", { path }),
  checkGithubLatest: () =>
    invoke<{ version: string; url: string; download_url?: string | null } | null>(
      "check_github_latest",
    ),
  getAiConfig: () => invoke<AiConfigView>("get_ai_config"),
  saveAiConfig: (args: {
    enabled: boolean;
    provider: string;
    baseUrl: string;
    model: string;
    allowCloudPaths: boolean;
    apiKey: string | null;
  }) => invoke<AiConfigView>("save_ai_config", args),
  aiRiskBrief: (request: Record<string, unknown>) =>
    invoke<string | null>("ai_risk_brief", { request }),
  aiExplain: (appName: string, publisher: string, items: AiExplainInput[]) =>
    invoke<AiExplainOutput[]>("ai_explain_items", { appName, publisher, items }),
  aiSummarizeReport: (request: Record<string, unknown>) =>
    invoke<string | null>("ai_summarize_report", { request }),
  aiParseIntent: (text: string, appNames: string[]) =>
    invoke<NlIntent>("ai_parse_intent", { text, appNames }),
  history: () => invoke<HistoryEntry[]>("list_cleanup_history"),
  exportHistoryCsv: () => invoke<string>("export_history_csv"),
  backupSessions: () => invoke<BackupSession[]>("list_backup_sessions"),
  deleteBackupSession: (name: string) => invoke("delete_backup_session", { name }),
  restoreSessionByName: (name: string) => invoke<string[]>("restore_session_by_name", { name }),
  suggestIgnoreRules: (publisher: string, paths: string[]) =>
    invoke<IgnoreSuggestion[]>("suggest_ignore_rules", { publisher, paths }),
  applyIgnoreSuggestions: (suggestions: IgnoreSuggestion[]) =>
    invoke<IgnoreLists>("apply_ignore_suggestions", { suggestions }),
  ignorePublisher: (name: string) => invoke<IgnoreLists>("ignore_publisher", { name }),
  ignoreAppName: (name: string) => invoke<IgnoreLists>("ignore_app_name", { name }),
  loadIgnore: () => invoke<IgnoreLists>("load_ignore"),
  diskUsage: () => invoke<DiskUsage>("disk_usage"),
  beginSizeEstimate: () => invoke("begin_size_estimate"),
  cancelSizeEstimate: () => invoke("cancel_size_estimate"),
  estimateDirSizeKb: (path: string) => invoke<number>("estimate_dir_size_kb", { path }),
  takePendingAnalyze: () => invoke<string | null>("take_pending_analyze"),
  beginInstallMonitor: () => invoke("begin_install_monitor"),
  endInstallMonitor: () => invoke<MonitorDiff>("end_install_monitor"),
  monitorDiffToItems: (diff: MonitorDiff) =>
    invoke<CleanupItem[]>("monitor_diff_to_items", { diff }),
  registerContextMenu: () => invoke("register_context_menu"),
  unregisterContextMenu: () => invoke("unregister_context_menu"),
  setStartupEnabled: (location: string, enabled: boolean) =>
    invoke("set_startup_enabled", { location, enabled }),
  setServiceStartDisabled: (name: string, disable: boolean) =>
    invoke("set_service_start_disabled", { name, disable }),
  setTaskEnabled: (name: string, enabled: boolean) =>
    invoke("set_task_enabled", { name, enabled }),
  listStartupItems: () => invoke<ManageItem[]>("list_startup_items"),
  listServices: () => invoke<ManageItem[]>("list_services"),
  listScheduledTasks: () => invoke<ManageItem[]>("list_scheduled_tasks"),
  appIconData: (displayIcon: string | null | undefined) =>
    invoke<string | null>("app_icon_data", { displayIcon: displayIcon ?? null }),
};
