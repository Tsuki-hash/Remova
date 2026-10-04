import type { CleanupItem } from "../types";

/** Pure estimate behind the scan action bar's size/attention numbers. */
export function summarizeCleanupPlan(items: CleanupItem[], selected: Set<string>) {
  const picked = items.filter(item => selected.has(item.path));
  const key = (path: string) => path.replaceAll("/", "\\").toLowerCase().replace(/\\+$/, "");
  const unique = new Map<string, CleanupItem>();
  for (const item of picked.filter(item => item.kind === "file" || item.kind === "dir")) {
    if (!unique.has(key(item.path))) unique.set(key(item.path), item);
  }
  const directories = new Set([...unique].filter(([, item]) => item.kind === "dir").map(([path]) => path));
  const roots = [...unique].filter(([path]) => {
    for (let end = path.lastIndexOf("\\"); end >= 0; end = path.lastIndexOf("\\", end - 1)) {
      if (directories.has(path.slice(0, end))) return false;
      if (end === 0) break;
    }
    return true;
  });
  let knownKb = 0, unknown = 0;
  for (const [, item] of roots) {
    if (item.size_kb != null && Number.isFinite(item.size_kb) && item.size_kb >= 0) knownKb += item.size_kb;
    else unknown++;
  }
  return { count: picked.length, knownKb, unknown,
    review: picked.filter(item => item.risk !== "low" || item.confidence !== "confirmed"
      || item.shared || item.user_data || item.user_library).length,
    registry: picked.filter(item => item.kind === "registry").length,
    path: picked.filter(item => item.kind === "path").length };
}
