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
  kind: "file" | "dir" | "registry";
  score: number;
  confidence: "confirmed" | "suspected";
  risk: "low" | "medium" | "high";
  reason: string;
  evidence: Evidence[];
  shared?: boolean;
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
