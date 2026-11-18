import type { DirSizeRow } from "../types";

export const DISK_RADAR_SNAPSHOT_KEY = "remova.diskRadar.snapshot.v1";

type SnapshotRow = {
  key: string;
  name: string;
  size: number;
  capped: boolean;
};

export type DiskRadarSnapshot = {
  volume: string;
  scope: string;
  takenAt: number;
  rows: SnapshotRow[];
};

function rowKey(row: DirSizeRow) {
  const path = row.path.slice(2).replaceAll("/", "\\").replace(/\\+$/, "");
  return `${row.aggregate ? "total" : "folder"}:${path}`;
}

export function snapshotDiskRadar(volume: string, scope: string, rows: DirSizeRow[]): DiskRadarSnapshot {
  return {
    volume,
    scope,
    takenAt: Date.now(),
    rows: rows.map(row => ({ key: rowKey(row), name: row.name, size: row.size_kb, capped: Boolean(row.capped) })),
  };
}

export function readDiskRadarSnapshot(storage: Pick<Storage, "getItem">) {
  const raw = storage.getItem(DISK_RADAR_SNAPSHOT_KEY);
  if (!raw) return null;
  if (raw.length > 256 * 1024) throw new Error("invalid disk radar snapshot");
  const value = JSON.parse(raw) as DiskRadarSnapshot | null;
  if (!value || typeof value.volume !== "string" || typeof value.scope !== "string" ||
      !Number.isSafeInteger(value.takenAt) || !Array.isArray(value.rows) || value.rows.length > 40 ||
      value.rows.some(row => !row || typeof row.key !== "string" || typeof row.name !== "string" ||
        !Number.isSafeInteger(row.size) || row.size < 0 || typeof row.capped !== "boolean")) {
    throw new Error("invalid disk radar snapshot");
  }
  return value;
}

export type DiskRadarChange = {
  key: string;
  name: string;
  before?: number;
  after?: number;
  delta?: number;
  row?: DirSizeRow;
};

export function compareDiskRadar(snapshot: DiskRadarSnapshot, volume: string, scope: string, current: DirSizeRow[]): DiskRadarChange[] | null {
  if (snapshot.volume !== volume || snapshot.scope !== scope) return null;
  const previous = new Map(snapshot.rows.map(row => [row.key, row]));
  const matches: DiskRadarChange[] = current.map(row => {
    const key = rowKey(row);
    const before = previous.get(key);
    previous.delete(key);
    const delta = before && !before.capped && !row.capped ? row.size_kb - before.size : undefined;
    return { key, name: row.name, before: before?.size, after: row.size_kb, delta, row };
  });
  return matches.concat(Array.from(previous.values()).map(row => ({
    key: row.key, name: row.name, before: row.size,
  }))).sort((a, b) => Math.abs(b.delta ?? 0) - Math.abs(a.delta ?? 0));
}
