import type { CleanupItem, IdleApp, IdleScanResult, OrphanScanResult, ScanCompleteness, ScopedScanResult } from "../types";

export const COMPLETE_SCAN: ScanCompleteness = {
  status: "complete",
  roots_scanned: 0,
  failed_roots: [],
};

/** Older backends remain readable, but their completeness is unknown—not complete. */
export const LEGACY_SCAN: ScanCompleteness = {
  status: "unknown",
  roots_scanned: 0,
  failed_roots: ["legacy_payload"],
};

function normalizeScan<T>(raw: { items: T[]; omitted: number; completeness?: ScanCompleteness } | T[] | null | undefined) {
  if (Array.isArray(raw)) return { items: raw, omitted: 0, completeness: LEGACY_SCAN };
  return {
    items: raw?.items ?? [],
    omitted: raw?.omitted ?? 0,
    completeness: raw?.completeness ?? LEGACY_SCAN,
  };
}

export function normalizeOrphanScan(raw: OrphanScanResult | CleanupItem[] | null | undefined) {
  return normalizeScan<CleanupItem>(raw);
}

export function normalizeIdleScan(raw: IdleScanResult | IdleApp[] | null | undefined) {
  const normalized = normalizeScan<IdleApp>(raw);
  return normalized;
}

/**
 * Accept the scoped payload and, while the backend field rollout is in flight,
 * a bare item array. The legacy array is explicitly incomplete: crashing is
 * worse, but displaying it as a complete scan would be a false clean result.
 */
export function normalizeScopedScan(raw: ScopedScanResult | CleanupItem[] | null | undefined): ScopedScanResult {
  if (Array.isArray(raw)) return { items: raw, omitted: 0, degraded: ["legacy_payload"] };
  return { items: raw?.items ?? [], omitted: raw?.omitted ?? 0, degraded: raw?.degraded ?? [] };
}

export type ScannedGroup = { bucket: string; items: CleanupItem[] };

/** Unknown/missing buckets sort last but still get a named header. */
function bucketRank(bucket: string): number {
  return bucket ? 0 : 1;
}

/**
 * Group rows by backend bucket and sort by measured size descending inside each
 * group. Backend insertion order put a multi-GB browser cache below dozens of
 * KB-sized npm directories, and the panel only shows about seven rows per screen.
 */
export function groupScannedItems(items: CleanupItem[]): ScannedGroup[] {
  const byBucket = new Map<string, CleanupItem[]>();
  for (const it of items) {
    const key = it.bucket ?? "";
    const list = byBucket.get(key);
    if (list) list.push(it);
    else byBucket.set(key, [it]);
  }
  return [...byBucket.entries()]
    .map(([bucket, list]) => ({
      bucket,
      items: [...list].sort((a, b) => (b.size_kb ?? -1) - (a.size_kb ?? -1) || a.path.localeCompare(b.path)),
    }))
    .sort((a, b) => {
      const max = (g: ScannedGroup) => g.items.reduce((m, it) => Math.max(m, it.size_kb ?? -1), -1);
      return bucketRank(a.bucket) - bucketRank(b.bucket) || max(b) - max(a) || a.bucket.localeCompare(b.bucket);
    });
}
