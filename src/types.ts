export type InstalledApp = {
  name: string;
  version: string;
  publisher: string;
  install_location: string;
  uninstall_string: string;
  quiet_uninstall_string: string;
  source: string;
  registry_key: string;
  estimated_size_kb: number;
  install_date: string;
  display_icon: string;
};

export type Evidence = {
  code: string;
  label: string;
  weight: number;
  detail: string;
};

export type CleanupItem = {
  path: string;
  kind: "file" | "dir" | "registry" | "path";
  score: number;
  confidence: "confirmed" | "suspected";
  risk: "low" | "medium" | "high";
  reason: string;
  evidence: Evidence[];
  shared?: boolean;
  /** Hosts a registered Windows service image — keep-class, never default-select. */
  service_bound?: boolean;
  /** Likely user documents/downloads — never auto-select (SOP). */
  user_data?: boolean;
  /** Under a user library folder but not the root — confirm only, never default-select. */
  user_library?: boolean;
  /** Best-effort size in KB (file/dir only; null/omitted for registry/path). */
  size_kb?: number | null;
  /** `size_kb` hit a walk bound and is a verified floor — display `>= N`, never as exact. */
  size_capped?: boolean;
  /** Display bucket from backend (programFiles/configFiles/…). */
  bucket?: string | null;
};

export type ScanResult = {
  app_name: string;
  /** Backend-built `source\0registry_key\0name` identity; absent on synthetic
   * scans (monitor diff / orphan) which fall back to name comparison. */
  app_key?: string;
  items: CleanupItem[];
  /** Stable codes for bounded scans that dropped matches. */
  degraded?: string[];
};

export type ItemDetail = {
  path: string;
  kind: string;
  status: string;
  message: string;
};

/**
 * Result of a scoped specialty scan (`scan_installer_caches` / `scan_tool_caches`).
 *
 * `omitted` is the number of rows the backend phase cap dropped — without it the
 * panel implies the list is exhaustive. `degraded` names the roots the scanner
 * could not read, which is what separates "nothing found" from "could not look".
 */
export type ScopedScanResult = {
  items: CleanupItem[];
  omitted: number;
  degraded: string[];
};

/** Orphan/idle scans used to truncate at their cap with no signal. */
export type OrphanScanResult = { items: CleanupItem[]; omitted: number };
export type IdleScanResult = { items: IdleApp[]; omitted: number };

/** An MSI product whose install directory is gone but whose record remains. */
export type BrokenInstall = {
  app: InstalledApp;
  guid: string;
  reason: string;
};

/** M2: keys removed by the broken-record cleanup, and where the export lives. */
export type BrokenCleanupReport = {
  removed: string[];
  failed: string[];
  backup_dir: string;
};

/** M3: a firewall rule whose App= executable no longer exists. */
export type FirewallRule = {
  value_name: string;
  rule_name: string;
  app_path: string;
  direction: string;
  action: string;
  protocol: string;
  reason: string;
};
export type FirewallScanResult = { rules: FirewallRule[]; omitted: number };
export type FirewallCleanupReport = {
  removed: string[];
  failed: string[];
  backup_dir: string;
};

/** Disk-space overview: one directory reparse point (the link, not its target). */
export type JunctionEntry = {
  path: string;
  target: string | null;
  reachable: boolean;
};
export type JunctionScanResult = {
  entries: JunctionEntry[];
  omitted: number;
  degraded_roots: string[];
};

/** Disk-space overview: DISM component-store analysis (read-only). */
export type ComponentStoreAnalysis = {
  cleanup_recommended: boolean;
  explorer_size_bytes: number | null;
  actual_size_bytes: number | null;
};

/** Disk-space overview: judgement-table triage (read-only). */
export type TriageClass =
  | "delete_regenerable"
  | "via_dism"
  | "migrate_candidate"
  | "leave"
  | "show_only";
export type TriageEntry = {
  path: string;
  class: TriageClass;
  reason: string;
  size_kb: number | null;
  size_capped: boolean;
};
export type TriageScanResult = { entries: TriageEntry[]; omitted: number };

/** Disk-space overview: regenerable-layout cleanup (invariant 7, no backup). */
export type RegenerableItem = {
  path: string;
  reason: string;
  size_kb: number;
  size_capped: boolean;
  deletable: boolean;
  block_reason: string | null;
};
export type RegenerableScanResult = { items: RegenerableItem[]; omitted: number };
export type RegenerableSkipped = { path: string; message: string };
export type RegenerableCleanupReport = {
  cleaned: string[];
  skipped: RegenerableSkipped[];
  reclaimed_kb: number;
};

/** M4: global CLI package inventory (read-only; npm/pnpm/yarn). */
export type CliGlobalItem = {
  manager: string;
  name: string;
  version: string | null;
  path: string;
  size_kb: number;
  size_capped: boolean;
};
export type CliGlobalScanResult = {
  items: CliGlobalItem[];
  omitted: number;
  notes: string[];
};
export type NpmCleanupReport = {
  removed: string[];
  failed: string[];
  backup_dir: string;
};

/** M6.3: junction directory migration (four gated phases). */
export type MigrationPlan = {
  source: string;
  target: string;
  ok: boolean;
  rejections: string[];
  source_kb: number;
  file_count: number;
  target_free_kb: number;
};
export type MigrationCopyReport = {
  files: number;
  source_kb: number;
  files_capped: boolean;
  reparse_skipped: string[];
};
export type MigrationCutoverReport = {
  ok: boolean;
  junction: string;
  backup: string;
  rolled_back: boolean;
  error: string | null;
};
export type MigrationFinishReport = {
  removed_bak: boolean;
  missing_in_target: string[];
};
export type MigrationRollbackReport = {
  ok: boolean;
  message: string | null;
};
export type MigrationState =
  | "absent"
  | "idle"
  | "copied"
  | "cutover"
  | "finished"
  | "inconsistent";

/** M4: system traces — ConsentStore orphan (read-only this cycle). */
export type ConsentOrphan = {
  capability: string;
  key: string;
  exe_path: string;
};
export type TraceScanResult = {
  consent_orphans: ConsentOrphan[];
  omitted: number;
};

/** M4: portable-app candidates (read-only). */
export type PortableCandidate = {
  exe_path: string;
  rule_name: string;
};
export type PortableScanResult = {
  candidates: PortableCandidate[];
  omitted: number;
};

/** M4: processes the Restart Manager reports as holding a path. */
export type LockingProcess = { pid: number; name: string; path?: string };

/** M5: bounded, read-only registry key matches. */
export type RegistryCheckHit = { key: string; name: string; detail: string };
export type RegistryCheckResult = {
  hits: RegistryCheckHit[];
  scanned: number;
  truncated: boolean;
  timed_out: boolean;
};

export type CleanupReport = {
  app_name: string;
  /** Stable scan identity; absent on older/synthetic reports. */
  app_key?: string;
  dry_run: boolean;
  uninstall_command: string[];
  uninstall_message: string;
  deleted_planned: number;
  skipped: number;
  errors: string[];
  item_details: ItemDetail[];
};

export type FullCleanupReport = {
  app_name: string;
  /** Stable scan identity; absent on older/synthetic reports. */
  app_key?: string;
  dry_run: boolean;
  backup_dir: string;
  uninstall_ok: boolean;
  uninstall_message: string;
  deleted: number;
  failed: number;
  skipped: number;
  /** Reboot-delayed deletes (not in `deleted`). */
  delayed?: number;
  /** M5: size actually deleted, kept by the gate, and scheduled for reboot. */
  reclaimed_kb?: number;
  kept_kb?: number;
  delayed_kb?: number;
  aborted: boolean;
  /** MSI 1641 / global reboot gate: deletion postponed until restart. */
  reboot_pending?: boolean;
  reboot_gate_persisted?: boolean;
  restore_point_ok: boolean;
  restore_point_msg: string;
  errors: string[];
  item_details: ItemDetail[];
};

export type OfficialUninstallResult = {
  ok: boolean;
  message: string;
  had_command: boolean;
  /** MSI 1641: the uninstaller already initiated a restart. */
  reboot_initiated?: boolean;
  /** False when the 1641 gate could not be persisted across a relaunch. */
  reboot_gate_persisted?: boolean;
};

export type AiConfigView = {
  enabled: boolean;
  provider: string;
  base_url: string;
  model: string;
  allow_cloud_paths: boolean;
  has_api_key: boolean;
};

export type AiExplainOutput = {
  path: string;
  summary: string;
  suggest_check: boolean;
};

export type NlFilter = {
  name_like?: string | null;
  publisher?: string | null;
  size_gt_kb?: number | null;
  installed_after?: string | null;
};

/** Copilot / AI intent actions the UI knows how to plan for. */
export type NlIntentAction = "list" | "analyze" | "batch_uninstall" | "force_clean";

/**
 * Copilot / AI intent payload (single type for frontend + IPC).
 * The `action` crosses a model boundary, so an unrecognised value must stay representable —
 * `NlIntentAction | (string & {})` keeps literal autocomplete without claiming any string is valid.
 */
export type NlIntent = {
  action: NlIntentAction | (string & {});
  filter: NlFilter;
  include_leftovers: boolean;
  note: string;
};

export type IgnoreSuggestion = {
  kind: string;
  value: string;
  reason: string;
};

export type IdleEvidence = { code: string; detail: string };

export type IdleApp = {
  app: InstalledApp;
  idle_days: number;
  size_kb: number;
  evidence: IdleEvidence[];
  score: number;
};

export type DirSizeRow = {
  path: string;
  name: string;
  size_kb: number;
  parent: string;
  /** the walk was cut short (budget, depth cap, queue cap, unreadable level or
   * cancellation) — size is a floor, not a total */
  capped?: boolean;
  /** entries actually inspected; with `capped`, the floor was taken here */
  scanned?: number;
  /** a listed root's own total rather than one child directory */
  aggregate?: boolean;
};

/** Drill-down result: the ranked children plus what the list does not show. */
export type DirChildrenResult = {
  rows: DirSizeRow[];
  /** rows the caller will not see: children never walked plus rows dropped by
   * the row cap. A level cut by the queue cap is reported through `capped`
   * instead (its exact count is unknown). */
  omitted?: number;
  /** the listing is not the whole story: the parent scan was cut short, or a
   * child could not be measured in full (its number is a floor) */
  capped?: boolean;
};

export type HistoryEntry = {
  /** Runtime row id (content hash); not persisted on disk. */
  id: string;
  app_name: string;
  deleted: number;
  failed: number;
  skipped: number;
  delayed?: number;
  aborted: boolean;
  dry_run: boolean;
  backup_dir: string;
  created_at: string;
};

export type ManageStartType = "auto" | "manual" | "disabled" | "other";
export type ManageKind = "startup" | "service" | "task";

export type ManageItem = {
  name: string;
  detail: string;
  location: string;
  enabled: boolean;
  /** Which manage tab this row belongs to (presentation). */
  kind?: ManageKind | null;
  /** Live process/service run state — not the same as enabled/start type. */
  running?: boolean | null;
  /** Service Start type only. */
  start_type?: ManageStartType | null;
  /** Startup source: registry / folder / store / service. */
  source_label?: string | null;
  last_run?: string | null;
  next_run?: string | null;
  path?: string | null;
};

/** Matches ai.rs ExplainInput (string enums for IPC). */
export type AiExplainInput = {
  path: string;
  kind: string;
  confidence: string;
  risk: string;
  reason: string;
  evidence_labels: string[];
};

/**
 * Request body for `ai_risk_brief` — mirrors `ai.rs::RiskBriefInput`
 * (`#[serde(rename_all = "camelCase")]`).
 *
 * This payload used to be `Record<string, unknown>`, the only IPC body in
 * the project that bypassed the type system, so a Rust-side field rename only
 * surfaced at runtime (the field silently vanished from the JSON and the model
 * got a prompt with a hole in it). `apiContract.test.ts` now parses both sides
 * and fails on drift.
 */
export type RiskBriefRequest = {
  appName: string;
  publisher: string;
  action: string;
  itemCount: number;
  kindCounts: Record<string, number>;
  hasService: boolean;
  hasRunKey: boolean;
  hasSharedHint: boolean;
};

/** Request body for `ai_summarize_report` — mirrors `ai.rs::ReportBriefInput`. */
export type ReportBriefRequest = {
  appName: string;
  deleted: number;
  failed: number;
  skipped: number;
  aborted: boolean;
  backupDir: string;
  restorePointOk: boolean;
  topFailed: string[];
};
