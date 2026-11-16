export type BackupSessionStatus = "verified" | "pending_verification" | "unsealed" | "requires_admin" | "missing_metadata" | "unknown" | "invalid" | "legacy";

export type BackupSession = {
  name: string;
  folder?: string;
  size_kb: number | null;
  created_at: string;
  /** Compatibility with backends that only report seal verification. */
  sealed: boolean;
  status?: BackupSessionStatus;
};

export function backupSessionLabel(name: string, labels: { npm: string; firewall: string; fallback: string }): string {
  const generated = name.match(/^(?:\d+|\d{8}-\d{6})_([a-z\d_-]+)$/i);
  if (!generated) return name;
  const source = generated[1]!;
  if (/^npm-globals(?:_\d+)?$/.test(source)) return labels.npm;
  if (/^firewall-rules(?:_\d+)?$/.test(source)) return labels.firewall;
  return source.replace(/_+/g, " ").trim() || labels.fallback;
}

export function backupSessionStatus(session: BackupSession): BackupSessionStatus {
  return session.status ?? (session.sealed ? "verified" : "invalid");
}

export function canPreviewBackup(session: BackupSession): boolean {
  const status = backupSessionStatus(session);
  return status === "verified" || status === "pending_verification";
}

export function backupDeleteExpectation(session: BackupSession, previewVerified: boolean): "verified" | "unsealed" | null {
  const status = backupSessionStatus(session);
  if (status === "unsealed") return status;
  if (status === "verified" || (status === "pending_verification" && previewVerified)) return "verified";
  return null;
}
