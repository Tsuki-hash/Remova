export type BackupSessionStatus = "verified" | "pending_verification" | "unsealed" | "requires_admin" | "missing_metadata" | "unknown" | "invalid" | "legacy";

export type BackupSession = {
  name: string;
  size_kb: number | null;
  created_at: string;
  /** Compatibility with backends that only report seal verification. */
  sealed: boolean;
  status?: BackupSessionStatus;
};

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
