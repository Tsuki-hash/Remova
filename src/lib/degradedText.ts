import { t } from "../i18n";

/**
 * UX-2: the backend's `degraded` payloads are stable codes, not English
 * prose — `unreadable_root:<path>`, `downloads_root_missing`,
 * `userprofile_not_set`, `localappdata_not_set`. Unknown or older-rollout
 * strings fall back to the raw text, so a payload from a newer backend still
 * says something true instead of a generic placeholder.
 */
export function degradedText(raw: string): string {
  const L = t();
  if (raw.startsWith("unreadable_root:")) {
    const path = raw.slice("unreadable_root:".length);
    return L.degradedUnreadableRoot(path || "?");
  }
  if (raw.startsWith("pe_budget_exhausted:")) {
    const n = Number.parseInt(raw.slice("pe_budget_exhausted:".length), 10);
    return L.degradedPeBudgetExhausted(Number.isFinite(n) ? n : 0);
  }
  switch (raw) {
    case "downloads_root_missing":
      return L.degradedDownloadsRootMissing;
    case "userprofile_not_set":
      return L.degradedProfileNotSet;
    case "localappdata_not_set":
      return L.degradedLocalAppDataNotSet;
    case "home_dot_truncated":
      return L.degradedHomeDotTruncated;
    case "muicache_truncated":
      return L.degradedMuicacheTruncated;
    default:
      return raw;
  }
}
