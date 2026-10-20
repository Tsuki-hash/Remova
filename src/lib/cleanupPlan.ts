import type { CleanupItem } from "../types";

/**
 * The frontend mirror of `fsutil::path_key`: slashes, surrounding quotes,
 * trailing dots/spaces per segment, and casing all resolve to one identity.
 * The header's count and its byte total must agree on what "one path" means.
 */
export const pathKey = (path: string) =>
  path
    .replaceAll("/", "\\")
    .trim()
    .replace(/^"+|"+$/g, "")
    .toLowerCase()
    .replace(/\\+$/, "")
    .split("\\")
    .map(seg => (seg === "." || seg === ".." ? seg : seg.replace(/[. ]+$/, "")))
    .join("\\");

/** Pure estimate behind the scan action bar's size/attention numbers. */
export function summarizeCleanupPlan(items: CleanupItem[], selected: Set<string>) {
  const picked = items.filter(item => selected.has(item.path));
  // R38-7: `count` is deduplicated by path identity, while the backend's
  // plan/delete phases are not. Scanners never emit the same path twice (the
  // allow-list and `scanned` sets are keyed by exact path), so the two only
  // diverge on a hand-forged payload — if that ever changes, deduplicate the
  // backend instead of counting raw rows here.
  const uniquePicked = new Map<string, CleanupItem>();
  for (const item of picked) {
    const key = pathKey(item.path);
    if (!uniquePicked.has(key)) uniquePicked.set(key, item);
  }
  const unique = new Map<string, CleanupItem>();
  for (const item of uniquePicked.values()) {
    if (item.kind === "file" || item.kind === "dir") unique.set(pathKey(item.path), item);
  }
  const roots = pickRoots(unique);
  let knownKb = 0, unknown = 0;
  for (const [, item] of roots) {
    if (item.size_kb != null && Number.isFinite(item.size_kb) && item.size_kb >= 0) knownKb += item.size_kb;
    else unknown++;
  }
  return { count: uniquePicked.size, knownKb, unknown,
    review: picked.filter(item => item.risk !== "low" || item.confidence !== "confirmed"
      || item.shared || item.user_data || item.user_library).length,
    registry: picked.filter(item => item.kind === "registry").length,
    path: picked.filter(item => item.kind === "path").length };
}

/** Collapse nested directories: an item already inside another picked dir adds no bytes of its own. */
function pickRoots(unique: Map<string, CleanupItem>): [string, CleanupItem][] {
  const directories = new Set([...unique].filter(([, item]) => item.kind === "dir").map(([path]) => path));
  return [...unique].filter(([path]) => {
    for (let end = path.lastIndexOf("\\"); end >= 0; end = path.lastIndexOf("\\", end - 1)) {
      if (directories.has(path.slice(0, end))) return false;
      if (end === 0) break;
    }
    return true;
  });
}

/** UX-1: the same subtotal over EVERY scan-found row, not just the selected ones. */
export function summarizeScanTotal(items: CleanupItem[]) {
  const unique = new Map<string, CleanupItem>();
  for (const item of items.filter(item => item.kind === "file" || item.kind === "dir")) {
    if (!unique.has(pathKey(item.path))) unique.set(pathKey(item.path), item);
  }
  const roots = pickRoots(unique);
  let knownKb = 0, unknown = 0;
  for (const [, item] of roots) {
    if (item.size_kb != null && Number.isFinite(item.size_kb) && item.size_kb >= 0) knownKb += item.size_kb;
    else unknown++;
  }
  return { knownKb, unknown };
}
