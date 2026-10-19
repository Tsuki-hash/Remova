import type { CleanupItem, ScopedScanResult } from "../types";

/**
 * Accept the scoped payload and, while the backend field rollout is in flight,
 * a bare item array. Treating an array as "nothing omitted, nothing degraded"
 * would be a lie, but crashing the panel would be worse — the honest copy
 * appears as soon as the new fields are present.
 */
export function normalizeScopedScan(raw: ScopedScanResult | CleanupItem[] | null | undefined): ScopedScanResult {
  if (Array.isArray(raw)) return { items: raw, omitted: 0, degraded: [] };
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
      // Unmeasured (null/undefined) sizes sort to the bottom of their group
      // instead of being treated as zero and floating to the top.
      items: [...list].sort((a, b) => (b.size_kb ?? -1) - (a.size_kb ?? -1) || a.path.localeCompare(b.path)),
    }))
    .sort((a, b) => bucketRank(a.bucket) - bucketRank(b.bucket) || a.bucket.localeCompare(b.bucket));
}
