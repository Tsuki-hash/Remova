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
  /** Likely user documents/downloads — never auto-select (SOP). */
  user_data?: boolean;
  /** Under a user library folder but not the root — confirm only, never default-select. */
  user_library?: boolean;
  /** Best-effort size in KB (file/dir only; null/omitted for registry/path). */
  size_kb?: number | null;
  /** Display bucket from backend (programFiles/configFiles/…). */
  bucket?: string | null;
};

export type ScanResult = {
  app_name: string;
  items: CleanupItem[];
};

export type ItemDetail = {
  path: string;
  kind: string;
  status: string;
  message: string;
};

export type CleanupReport = {
  app_name: string;
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
  dry_run: boolean;
  backup_dir: string;
  uninstall_ok: boolean;
  uninstall_message: string;
  deleted: number;
  failed: number;
  skipped: number;
  /** Reboot-delayed deletes (not in `deleted`). */
  delayed?: number;
  aborted: boolean;
  restore_point_ok: boolean;
  restore_point_msg: string;
  errors: string[];
  item_details: ItemDetail[];
};

export type OfficialUninstallResult = {
  ok: boolean;
  message: string;
  had_command: boolean;
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
};

export type HistoryEntry = {
  /** Runtime row id (`L{line_no}`); not persisted on disk. */
  id: string;
  app_name: string;
  deleted: number;
  failed: number;
  skipped: number;
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
