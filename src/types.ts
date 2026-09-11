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
};

export type ScanResult = {
  app_name: string;
  items: CleanupItem[];
};
