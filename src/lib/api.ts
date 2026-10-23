/** Thin typed wrappers around Tauri invoke — single frontend data layer. */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { currentLang } from "../i18n";
import { trackNativeCall } from "./nativeActivity";
import type {
  AiConfigView,
  AiExplainInput,
  AiExplainOutput,
  BrokenCleanupReport,
  BrokenInstall,
  FirewallCleanupReport,
  FirewallScanResult,
  CleanupItem,
  CleanupReport,
  DirSizeRow,
  FullCleanupReport,
  HistoryEntry,
  IdleApp,
  IdleScanResult,
  IgnoreSuggestion,
  InstalledApp,
  LockingProcess,
  ManageItem,
  NlIntent,
  OfficialUninstallResult,
  OrphanScanResult,
  RegistryCheckResult,
  ReportBriefRequest,
  RiskBriefRequest,
  ScanResult,
  ScopedScanResult,
} from "../types";

export type CleanupSourceId =
  | "uninstall"
  | "orphan"
  | "monitor"
  | "copilot"
  | "installer"
  | "toolcache";

export type FullCleanupOptions = {
  dry_run: boolean;
  skip_official_uninstall: boolean;
  backup_enabled: boolean;
  /** Gate source: uninstall | orphan | monitor | copilot | installer | toolcache */
  cleanup_source?: CleanupSourceId;
  /**
   * Ask Windows for a restore point before deleting. The backend owns the
   * decision when the field is absent (`#[serde(default = "default_true")]`),
   * so every cleanup used to request one with no way to opt out — a request
   * Windows rate-limits to one per 24 hours, which made the (previously
   * undiagnosable) failure look permanent.
   */
  restore_point?: boolean;
};

export type IgnoreLists = { publishers: string[]; names: string[]; paths?: string[] };
export type DiskUsage = { free_gb: number; total_gb: number; drive?: string };
export type DriveInfo = {
  letter: string;
  free_gb: number;
  total_gb: number;
  is_system: boolean;
};
export type MonitorDiff = {
  added_files: string[];
  added_reg_values: string[];
  /** entries beyond the diff caps (honest truncation, backend-filled). */
  files_truncated?: number;
  reg_truncated?: number;
  /** the monitor walk hit its budget/depth cap — the diff may miss entries
   * that never changed; present it as incomplete, not authoritative. */
  walk_degraded?: boolean;
};
export type MonitorEndResult = { diff: MonitorDiff; items: CleanupItem[] };
export type VerifyRow = { path: string; kind: string; still_there: boolean; error?: string | null };
export type BackupSession = {
  name: string;
  size_kb: number;
  created_at: string;
  /** `false` = an aborted backup's leftover directory; it has no seal and can never be restored. */
  sealed: boolean;
};
export type AssociationScanProgress = { stage: string; found: number };
export type RestorePreview = {
  name: string; files: number; registry: number; path_entries: number;
  existing: number; unavailable: number;
  entries: { target: string; status: "available" | "existing" | "missing" | "blocked" }[];
};

function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return trackNativeCall(() => tauriInvoke<T>(command, args), command);
}

export const api = {
  onlineUpdateSupported: () => invoke<boolean>("online_update_supported"),
  setTrayLanguage: (lang: string) => invoke<void>("set_tray_language", { lang }),
  cancelCleanup: () => invoke<void>("cancel_cleanup"),
  listApps: () => invoke<InstalledApp[]>("list_installed_apps"),
  analyze: (app: InstalledApp, scanId?: number) => invoke<ScanResult>("analyze_associations", { app, scanId }),
  beginAssociationScan: () => invoke<number>("begin_association_scan"),
  cancelAssociationScan: (scanId: number) => invoke<boolean>("cancel_association_scan", { scanId }),
  associationScanProgress: (scanId: number) => invoke<AssociationScanProgress | null>("association_scan_progress", { scanId }),
  dryRun: (
    app: InstalledApp,
    items: CleanupItem[],
    options?: { cleanup_source?: CleanupSourceId },
  ) => invoke<CleanupReport>("run_cleanup_dry_run", { app, items, cleanupSource: options?.cleanup_source }),
  fullCleanup: (app: InstalledApp, items: CleanupItem[], options: FullCleanupOptions) =>
    invoke<FullCleanupReport>("run_full_cleanup", { app, items, options }),
  officialUninstall: (app: InstalledApp) =>
    invoke<OfficialUninstallResult>("run_official_uninstall", { app }),
  verifyLeftovers: (items: CleanupItem[]) =>
    invoke<VerifyRow[]>("verify_cleanup_leftovers", { items }),
  orphanScan: (scanId?: number) =>
    invoke<OrphanScanResult | CleanupItem[]>("scan_orphan_leftovers", { scanId }),
  /** M1: broken MSI install records (read-only). */
  brokenInstalls: () => invoke<BrokenInstall[]>("scan_broken_installs"),
  /** M2: export then remove a broken record (server re-verifies the record). */
  cleanupBrokenInstall: (appName: string, registryKey: string, guid: string) =>
    invoke<BrokenCleanupReport>("cleanup_broken_install", { appName, registryKey, guid }),
  /** M3: orphan firewall rules (read-only). */
  firewallOrphans: () => invoke<FirewallScanResult>("scan_firewall_orphans"),
  /** M3: export then delete rule values (server re-verifies each orphan). */
  deleteFirewallRules: (valueNames: string[]) =>
    invoke<FirewallCleanupReport>("delete_firewall_rules", { valueNames }),
  /** M4: name the processes holding a path (read-only). */
  diagnoseLockedPath: (path: string) =>
    invoke<LockingProcess[]>("diagnose_locked_path", { path }),
  /** M4: end one confirmed process after server-side image verification. */
  endLockingProcess: (pid: number, expectedPath: string, targetPath: string) =>
    invoke<void>("end_locking_process", { pid, expectedPath, targetPath }),
  /** M5: bounded read-only registry key search. */
  deepRegistryCheck: (appName: string) =>
    invoke<RegistryCheckResult>("deep_registry_check", { appName }),
  rankIdleApps: (scanId?: number) =>
    invoke<IdleScanResult | IdleApp[]>("rank_idle_apps", { scanId }),
  scanInstallerCaches: (scanId?: number) => invoke<ScopedScanResult>("scan_installer_caches", { scanId }),
  scanToolCaches: (scanId?: number) => invoke<ScopedScanResult>("scan_tool_caches", { scanId }),
  listLocalDrives: () => invoke<DriveInfo[]>("list_local_drives"),
  listTopDirSizes: (drive?: string, scanId?: number) =>
    invoke<DirSizeRow[]>("list_top_dir_sizes", { drive: drive ?? null, scanId }),
  listDirChildren: (path: string, scanId?: number) => invoke<DirSizeRow[]>("list_dir_children", { path, scanId }),
  isElevated: () => invoke<boolean>("is_elevated"),
  elevateRestart: (approved = false) => invoke("elevate_restart", { approved }),
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
  aiRiskBrief: (request: RiskBriefRequest) =>
    invoke<string | null>("ai_risk_brief", { request, locale: currentLang() }),
  aiExplain: (appName: string, publisher: string, items: AiExplainInput[]) =>
    invoke<AiExplainOutput[]>("ai_explain_items", { appName, publisher, items, locale: currentLang() }),
  aiSummarizeReport: (request: ReportBriefRequest) =>
    invoke<string | null>("ai_summarize_report", { request, locale: currentLang() }),
  aiParseIntent: (text: string, appNames: string[]) =>
    invoke<NlIntent>("ai_parse_intent", { text, appNames, locale: currentLang() }),
  history: () => invoke<HistoryEntry[]>("list_cleanup_history"),
  exportHistoryCsv: () => invoke<string>("export_history_csv"),
  deleteHistory: (ids: string[]) => invoke<number>("delete_cleanup_history", { ids }),
  clearHistory: () => invoke("clear_cleanup_history"),
  backupSessions: () => invoke<BackupSession[]>("list_backup_sessions"),
  previewRestore: (name: string) => invoke<RestorePreview>("preview_restore_session", { name }),
  deleteBackupSession: (name: string) => invoke("delete_backup_session", { name }),
  restoreSessionByName: (name: string) => invoke<string[]>("restore_session_by_name", { name }),
  suggestIgnoreRules: (publisher: string, paths: string[]) =>
    invoke<IgnoreSuggestion[]>("suggest_ignore_rules", { publisher, paths }),
  applyIgnoreSuggestions: (suggestions: IgnoreSuggestion[]) =>
    invoke<IgnoreLists>("apply_ignore_suggestions", { suggestions }),
  ignorePublisher: (name: string) => invoke<IgnoreLists>("ignore_publisher", { name }),
  ignoreAppName: (name: string) => invoke<IgnoreLists>("ignore_app_name", { name }),
  unignorePublisher: (name: string) => invoke<IgnoreLists>("unignore_publisher", { name }),
  unignoreAppName: (name: string) => invoke<IgnoreLists>("unignore_app_name", { name }),
  loadIgnore: () => invoke<IgnoreLists>("load_ignore"),
  diskUsage: () => invoke<DiskUsage>("disk_usage"),
  beginSizeEstimate: () => invoke("begin_size_estimate"),
  cancelSizeEstimate: () => invoke("cancel_size_estimate"),
  estimateDirSizeKb: (path: string) =>
    invoke<{ kb: number; capped: boolean }>("estimate_dir_size_kb", { path }),
  takePendingAnalyze: () => invoke<string | null>("take_pending_analyze"),
  beginInstallMonitor: (scanId?: number) => invoke<void>("begin_install_monitor", { scanId }),
  endInstallMonitor: (scanId?: number) => invoke<MonitorEndResult>("end_install_monitor", { scanId }),
  setStartupEnabled: (location: string, enabled: boolean) =>
    invoke("set_startup_enabled", { location, enabled }),
  setServiceStartDisabled: (name: string, disable: boolean) =>
    invoke("set_service_start_disabled", { name, disable }),
  /** Stop/start a Windows service (not the same as disable start type). */
  setServiceRunning: (name: string, run: boolean) =>
    invoke("set_service_running", { name, run }),
  setTaskEnabled: (name: string, enabled: boolean) =>
    invoke("set_task_enabled", { name, enabled }),
  listStartupItems: (scanId?: number) => invoke<ManageItem[]>("list_startup_items", { scanId }),
  listServices: (scanId?: number) => invoke<ManageItem[]>("list_services", { scanId }),
  listScheduledTasks: (scanId?: number) => invoke<ManageItem[]>("list_scheduled_tasks", { scanId }),
  appIconData: (displayIcon: string | null | undefined) =>
    invoke<string | null>("app_icon_data", { displayIcon: displayIcon ?? null }),
};
