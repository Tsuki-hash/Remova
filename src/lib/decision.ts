/** Decision chips & leftover risk buckets 鈥?only use data we already have. */
import type { CleanupItem, InstalledApp } from "../types";

export type ChipTone = "accent" | "warn" | "muted" | "danger" | "ok";

export type DecisionChip = {
  id: string;
  label: string;
  tone: ChipTone;
  title?: string;
};

/** Parse install_date strings like 20240115 / 2024/01/15 / 1/15/2024. */
export function parseInstallDate(raw: string | undefined | null): Date | null {
  const s = (raw || "").trim();
  if (!s) return null;
  const ymd = s.match(/^(\d{4})[-/.]?(\d{2})[-/.]?(\d{2})$/);
  if (ymd) {
    const d = new Date(Number(ymd[1]), Number(ymd[2]) - 1, Number(ymd[3]));
    return Number.isNaN(d.getTime()) ? null : d;
  }
  const mdy = s.match(/^(\d{1,2})[-/.](\d{1,2})[-/.](\d{4})$/);
  if (mdy) {
    const d = new Date(Number(mdy[3]), Number(mdy[1]) - 1, Number(mdy[2]));
    return Number.isNaN(d.getTime()) ? null : d;
  }
  const d = new Date(s);
  return Number.isNaN(d.getTime()) ? null : d;
}

export function isRecentInstall(raw: string | undefined | null, days = 30, now = Date.now()): boolean {
  const d = parseInstallDate(raw);
  if (!d) return false;
  const age = now - d.getTime();
  return age >= 0 && age <= days * 24 * 60 * 60 * 1000;
}

/** Heuristic uninstall-candidate score from size + install age only (no fake usage). */
export function recommendScore(
  app: InstalledApp,
  sizeKb: number,
  now = Date.now(),
): number {
  let score = 0;
  if (sizeKb >= 2 * 1024 * 1024) score += 50;
  else if (sizeKb >= 1024 * 1024) score += 35;
  else if (sizeKb >= 500 * 1024) score += 20;
  const d = parseInstallDate(app.install_date);
  if (d) {
    const days = (now - d.getTime()) / (24 * 60 * 60 * 1000);
    if (days >= 365) score += 30;
    else if (days >= 180) score += 20;
    else if (days >= 90) score += 12;
  }
  const hasCmd = Boolean((app.quiet_uninstall_string || app.uninstall_string || "").trim());
  if (!hasCmd) score = Math.min(score, 10);
  return score;
}

export function isRecommendedCleanup(
  app: InstalledApp,
  sizeKb: number,
  now = Date.now(),
): boolean {
  const hasCmd = Boolean((app.quiet_uninstall_string || app.uninstall_string || "").trim());
  if (!hasCmd) return false;
  if (sizeKb >= 2 * 1024 * 1024) return true;
  if (sizeKb >= 1024 * 1024) {
    const d = parseInstallDate(app.install_date);
    if (!d) return false;
    return now - d.getTime() >= 90 * 24 * 60 * 60 * 1000;
  }
  return false;
}

/** Fact-only health label 鈥?never claims leftovers without a scan. */
export function appHealth(
  app: InstalledApp,
  sizeKb: number,
  L: { healthOk: string; healthAttention: string },
): { id: "ok" | "attention"; label: string } {
  const hasCmd = Boolean((app.quiet_uninstall_string || app.uninstall_string || "").trim());
  if (!hasCmd || sizeKb >= 1024 * 1024) return { id: "attention", label: L.healthAttention };
  return { id: "ok", label: L.healthOk };
}

/** Factual cleanup progress from report counters 鈥?no marketing 100%. */
export function cleanupProgress(deleted: number, failed: number, skipped: number): {
  handled: number;
  total: number;
  pct: number;
  complete: boolean;
} {
  const total = deleted + failed + skipped;
  const handled = deleted;
  const pct = total === 0 ? 0 : Math.round((handled / total) * 100);
  return { handled, total, pct, complete: failed === 0 && total > 0 };
}

/** 1鈥? chips under the app name; never invent usage we do not track. */
export function decisionChips(
  app: InstalledApp,
  sizeKb: number,
  L: {
    chipLarge: string;
    chipRecent: string;
    chipStore: string;
    chipDesktop: string;
    chipNoUninstall: string;
    chipHasPath: string;
    chipRecommend?: string;
  },
  opts?: { largeKb?: number; recentDays?: number; now?: number; compact?: boolean },
): DecisionChip[] {
  const largeKb = opts?.largeKb ?? 500 * 1024; // 500 MB
  const recentDays = opts?.recentDays ?? 30;
  const now = opts?.now ?? Date.now();
  const compact = opts?.compact ?? false;
  const out: DecisionChip[] = [];
  const hasCmd = Boolean((app.quiet_uninstall_string || app.uninstall_string || "").trim());

  if (!hasCmd) {
    out.push({ id: "no-uninstall", label: L.chipNoUninstall, tone: "warn" });
  }
  if (L.chipRecommend && isRecommendedCleanup(app, sizeKb, now)) {
    out.push({ id: "recommend", label: L.chipRecommend, tone: "accent" });
  }
  if (sizeKb >= largeKb) {
    out.push({
      id: "large",
      label: L.chipLarge,
      tone: "accent",
      title: `${sizeKb} KB`,
    });
  }
  // List rows: at most one high-signal chip 鈥?full metadata lives in detail panel.
  if (compact) {
    const priority = ["no-uninstall", "recommend", "large"] as const;
    for (const id of priority) {
      const hit = out.find((c) => c.id === id);
      if (hit) return [hit];
    }
    return [];
  }
  if (isRecentInstall(app.install_date, recentDays, now)) {
    out.push({
      id: "recent",
      label: L.chipRecent,
      tone: "ok",
      title: app.install_date,
    });
  }
  if (out.length < 3) {
    if (app.source === "Store") {
      out.push({ id: "store", label: L.chipStore, tone: "muted" });
    } else if (app.source) {
      out.push({ id: "desktop", label: L.chipDesktop, tone: "muted" });
    }
  }
  if (out.length < 3 && app.install_location?.trim()) {
    out.push({ id: "path", label: L.chipHasPath, tone: "muted", title: app.install_location });
  }
  return out.slice(0, 3);
}

export type LeftoverBucket = {
  id: "safe" | "suggest" | "keep";
  count: number;
};

export type LeftoverSummary = {
  total: number;
  safe: number;
  suggest: number;
  keep: number;
  byKind: { kind: string; count: number }[];
};

/** Safe = confirmed, not high risk, not shared, not user data. Keep = high risk or shared. */
export function bucketItem(it: CleanupItem): "safe" | "suggest" | "keep" {
  if (defaultSelectable(it)) return "safe";
  if (isKeepItem(it)) return "keep";
  return "suggest";
}

/** Keep bucket predicate (high risk / shared / user data) 鈥?single source for UI filters. */
export function isKeepItem(it: CleanupItem): boolean {
  return it.risk === "high" || Boolean(it.shared) || Boolean(it.user_data);
}

export type RiskTier = "low" | "medium" | "high";

/** Highest risk among picked items 鈥?used to label confirms. */
export function maxRiskOf(items: CleanupItem[]): RiskTier {
  if (items.some((it) => it.risk === "high")) return "high";
  if (items.some((it) => it.risk === "medium")) return "medium";
  return "low";
}

export function riskTierLabel(
  tier: RiskTier,
  L: { riskTierSafe: string; riskTierLow: string; riskTierMedium: string; riskTierHigh: string },
): string {
  if (tier === "high") return L.riskTierHigh;
  if (tier === "medium") return L.riskTierMedium;
  // Safe only when nothing is medium/high 鈥?callers pass maxRisk so low 鈮?low-risk items only.
  return L.riskTierLow;
}

/** Suggest = not default-selectable and not keep. */
export function isSuggestItem(it: CleanupItem): boolean {
  return !defaultSelectable(it) && !isKeepItem(it);
}

/** Single source of truth: should this leftover be pre-checked for cleanup? */
export function defaultSelectable(it: CleanupItem): boolean {
  return (
    it.confidence === "confirmed" &&
    it.risk !== "high" &&
    !it.shared &&
    !it.user_data &&
    !it.user_library
  );
}

export function summarizeLeftovers(items: CleanupItem[]): LeftoverSummary {
  const byKind = new Map<string, number>();
  let safe = 0;
  let suggest = 0;
  let keep = 0;
  for (const it of items) {
    byKind.set(it.kind, (byKind.get(it.kind) || 0) + 1);
    const b = bucketItem(it);
    if (b === "safe") safe += 1;
    else if (b === "suggest") suggest += 1;
    else keep += 1;
  }
  return {
    total: items.length,
    safe,
    suggest,
    keep,
    byKind: [...byKind.entries()]
      .map(([kind, count]) => ({ kind, count }))
      .sort((a, b) => b.count - a.count),
  };
}

/** Human one-line reason for a leftover row (falls back to backend reason). */
export function leftoverReasonLine(
  it: CleanupItem,
  L: {
    reasonBelongs: string;
    reasonShared: string;
    reasonHigh: string;
    reasonSuspect: string;
    reasonUserData: string;
    reasonUserLibrary: string;
  },
): string {
  if (it.user_data) return L.reasonUserData;
  if (it.user_library) return L.reasonUserLibrary;
  if (it.shared) return L.reasonShared;
  if (it.risk === "high") return L.reasonHigh;
  if (it.confidence === "confirmed") return L.reasonBelongs;
  return L.reasonSuspect;
}

/** Labels for backend gate skip codes shown in cleanup report details. */
export type GateReasonLabels = {
  reasonUserData: string;
  reasonShared: string;
  reasonIgnored: string;
  reasonPathProtected: string;
  reasonSafetyGate: string;
  reasonNotAssociated: string;
  reasonPathMissing: string;
  reasonNotInPath: string;
  reasonRebootDelete: string;
};

/** Map policy/executor skip codes to user-facing Chinese/English copy. */
export function gateReasonText(message: string, L: GateReasonLabels): string {
  const raw = message || "";
  const m = raw.toLowerCase();
  if (!m) return message;
  if (m.includes("user_data") || m.includes("user data") || m.includes("sync conflict") || m.includes("sync_conflict")) {
    return L.reasonUserData;
  }
  if (m.includes("shared runtime") || m === "shared") return L.reasonShared;
  if (m.includes("ignored")) return L.reasonIgnored;
  if (m.includes("protected path")) return L.reasonPathProtected;
  if (m.includes("safety gate") || m.includes("failed safety")) return L.reasonSafetyGate;
  if (m.includes("not associated")) return L.reasonNotAssociated;
  if (m.includes("path missing")) return L.reasonPathMissing;
  if (m.includes("not found in path")) return L.reasonNotInPath;
  if (m.includes("reboot delete")) return L.reasonRebootDelete;
  // F-R6-09: common Chinese backend / reason copy (same buckets as the codes above).
  if (raw.includes("用户数据") || raw.includes("用户资料")) return L.reasonUserData;
  if (raw.includes("共享运行") || raw.includes("共享组件") || raw.includes("共享库")) {
    return L.reasonShared;
  }
  if (raw.includes("忽略列表") || raw.includes("已在忽略") || raw.includes("已忽略")) {
    return L.reasonIgnored;
  }
  if (raw.includes("系统 PATH") || raw.includes("PATH 条目") || raw.includes("受保护路径") || raw.includes("受保护的路径")) {
    return L.reasonPathProtected;
  }
  if (raw.includes("安全门") || raw.includes("删除安全")) return L.reasonSafetyGate;
  if (raw.includes("无可靠关联") || raw.includes("未关联") || raw.includes("关联不上")) {
    return L.reasonNotAssociated;
  }
  if (raw.includes("路径不存在") || raw.includes("路径已不存在") || raw.includes("路径缺失")) {
    return L.reasonPathMissing;
  }
  if (raw.includes("PATH 中已不存在") || raw.includes("PATH 已不存在") || raw.includes("不在 PATH")) {
    return L.reasonNotInPath;
  }
  if (raw.includes("重启后删除") || raw.includes("已安排重启") || raw.includes("重启删除")) {
    return L.reasonRebootDelete;
  }
  return message;
}

/** Last path segment as a human "origin" hint (folder name, not full path). */
export function originLabel(path: string): string {
  const parts = (path || "")
    .replace(/\//g, "\\")
    .split("\\")
    .filter((s) => s && !/^[A-Za-z]:$/.test(s));
  if (parts.length === 0) return path || "-";
  let last = parts[parts.length - 1];
  // File path 鈫?use parent folder as the origin hint.
  if (parts.length >= 2 && /\.[A-Za-z0-9]{1,12}$/.test(last) && !last.startsWith(".")) {
    last = parts[parts.length - 2];
  }
  return last || path || "-";
}

export type OriginGroup = {
  origin: string;
  count: number;
  safe: number;
  suggest: number;
  keep: number;
  items: CleanupItem[];
};

/**
 * Group leftovers by path leaf (suspected software folder).
 * Orphans have no uninstall entry 鈥?never invent a product name.
 */
export function groupByOrigin(items: CleanupItem[]): OriginGroup[] {
  const map = new Map<string, CleanupItem[]>();
  for (const it of items) {
    const key = originLabel(it.path).toLowerCase();
    const list = map.get(key);
    if (list) list.push(it);
    else map.set(key, [it]);
  }
  return [...map.values()]
    .map((list) => {
      let safe = 0;
      let suggest = 0;
      let keep = 0;
      for (const it of list) {
        const b = bucketItem(it);
        if (b === "safe") safe += 1;
        else if (b === "suggest") suggest += 1;
        else keep += 1;
      }
      return {
        origin: originLabel(list[0].path),
        count: list.length,
        safe,
        suggest,
        keep,
        items: list,
      };
    })
    .sort((a, b) => b.count - a.count || a.origin.localeCompare(b.origin));
}

/** Flatten groups back into a stable display order for tables. */
export function flattenOriginGroups(groups: OriginGroup[]): CleanupItem[] {
  return groups.flatMap((g) => g.items);
}
