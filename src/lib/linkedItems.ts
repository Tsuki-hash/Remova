/** Linked-item buckets for the right detail panel (path heuristics only). */
import type { CleanupItem, InstalledApp } from "../types";

export type LinkedBucketId =
  | "programFiles"
  | "configFiles"
  | "registry"
  | "shortcuts"
  | "startup"
  | "other";

export type LinkedBucket = {
  id: LinkedBucketId;
  count: number;
  /** Sum of size_kb for file/dir members; null when no measurable sizes. */
  sizeKb: number | null;
};

const BUCKET_ORDER: LinkedBucketId[] = [
  "programFiles",
  "configFiles",
  "registry",
  "shortcuts",
  "startup",
  "other",
];

function isLnk(path: string): boolean {
  return /\.lnk$/i.test(path || "");
}

function isStartupPath(path: string): boolean {
  const p = (path || "").toLowerCase().replace(/\//g, "\\");
  return (
    p.includes("\\currentversion\\run") ||
    p.includes("\\startup") ||
    p.includes("\\start menu\\programs\\startup")
  );
}

function isConfigPath(path: string): boolean {
  const p = (path || "").toLowerCase().replace(/\//g, "\\");
  return (
    p.includes("\\appdata\\") ||
    p.includes("\\programdata\\") ||
    p.includes("\\application data\\")
  );
}

function isUnderInstall(path: string, install: string): boolean {
  const a = (path || "").toLowerCase().replace(/\//g, "\\");
  const b = (install || "").toLowerCase().replace(/\//g, "\\").replace(/\\+$/, "");
  if (!b) return false;
  return a === b || a.startsWith(`${b}\\`);
}

/** Classify one leftover into a display bucket (presentation only). */
export function classifyItem(it: CleanupItem, app: InstalledApp): LinkedBucketId {
  // Prefer backend bucket when present (1.1).
  const b = (it.bucket || "").trim();
  if (
    b === "programFiles" ||
    b === "configFiles" ||
    b === "registry" ||
    b === "shortcuts" ||
    b === "startup" ||
    b === "other"
  ) {
    return b;
  }
  const kind = (it.kind || "").toLowerCase();
  const path = it.path || "";

  if (kind === "file" && isLnk(path)) {
    return isStartupPath(path) ? "startup" : "shortcuts";
  }
  if (kind === "registry") {
    return isStartupPath(path) ? "startup" : "registry";
  }
  if (kind === "path") return "other";
  if (kind === "file" || kind === "dir") {
    if (isStartupPath(path)) return "startup";
    if (isConfigPath(path)) return "configFiles";
    if (isUnderInstall(path, app.install_location) || /\\program files( \(x86\))?\\/i.test(path)) {
      return "programFiles";
    }
    return "configFiles";
  }
  return "other";
}

/** Aggregate leftovers into ordered buckets (empty buckets omitted). */
export function buildLinkedBuckets(
  items: CleanupItem[],
  app: InstalledApp,
): LinkedBucket[] {
  const map = new Map<LinkedBucketId, { count: number; sizeSum: number; hasSize: boolean }>();
  for (const it of items) {
    const id = classifyItem(it, app);
    const cur = map.get(id) ?? { count: 0, sizeSum: 0, hasSize: false };
    cur.count += 1;
    const kind = (it.kind || "").toLowerCase();
    if (kind === "file" || kind === "dir") {
      const kb = it.size_kb;
      if (typeof kb === "number" && kb > 0) {
        cur.sizeSum += kb;
        cur.hasSize = true;
      }
    }
    map.set(id, cur);
  }
  return BUCKET_ORDER.flatMap((id) => {
    const v = map.get(id);
    if (!v || v.count === 0) return [];
    return [
      {
        id,
        count: v.count,
        sizeKb: v.hasSize ? v.sizeSum : null,
      },
    ];
  });
}

/** Items that belong to a bucket (or all when filter is null). */
export function filterItemsByBucket(
  items: CleanupItem[],
  app: InstalledApp,
  bucket: LinkedBucketId | null | undefined,
): CleanupItem[] {
  if (!bucket) return items;
  return items.filter((it) => classifyItem(it, app) === bucket);
}

export function linkedBucketLabelKey(id: LinkedBucketId): string {
  switch (id) {
    case "programFiles":
      return "linkedProgramFiles";
    case "configFiles":
      return "linkedConfigFiles";
    case "registry":
      return "linkedRegistry";
    case "shortcuts":
      return "linkedShortcuts";
    case "startup":
      return "linkedStartup";
    default:
      return "linkedOther";
  }
}

export function linkedBucketIcon(id: LinkedBucketId): string {
  switch (id) {
    case "programFiles":
      return "📁";
    case "configFiles":
      return "⚙";
    case "registry":
      return "🗄";
    case "shortcuts":
      return "🔗";
    case "startup":
      return "▶";
    default:
      return "•";
  }
}
