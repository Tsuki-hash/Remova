export type BackupSessionStatus = "verified" | "unsealed" | "requires_admin" | "invalid" | "legacy";

export type BackupSession = {
  name: string;
  size_kb: number;
  created_at: string;
  /** Compatibility with backends that only report seal verification. */
  sealed: boolean;
  status?: BackupSessionStatus;
};

export function backupSessionStatus(session: BackupSession): BackupSessionStatus {
  return session.status ?? (session.sealed ? "verified" : "invalid");
}
