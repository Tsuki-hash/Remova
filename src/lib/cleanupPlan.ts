import type { CleanupItem } from "../types";

/**
 * The frontend mirror of `fsutil::path_key`: slashes, surrounding quotes,
 * trailing dots/spaces per segment, and casing all resolve to one identity.
 * Used for file size estimates, where overlapping file targets count once.
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

/** One selectable action in the current scan. Keep the server's spelling. */
export function cleanupItemKey(item: Pick<CleanupItem, "kind" | "path">): string {
  return `${item.kind}\0${item.path}`;
}

/** Pure estimate behind the scan action bar's size/attention numbers. */
export function summarizeCleanupPlan(items: CleanupItem[], selected: Set<string>) {
  const picked = items.filter(item => selected.has(cleanupItemKey(item)));
  return { count: picked.length, ...summarizeScanTotal(picked),
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

/** the same subtotal over EVERY scan-found row, not just the selected ones. */
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
