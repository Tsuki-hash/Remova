/** Heuristics for hiding system noise on manage lists. */

export type ManageLike = {
  name: string;
  detail?: string;
  location?: string;
};

const MICROSOFT_TOKENS = [
  "microsoft",
  "windows defender",
  "windows update",
  "windows security",
  "windows time",
  "windows push",
];

const SYSTEM_NAME_PREFIXES = [
  "wpn",
  "w32",
  "wuauserv",
  "windefend",
  "winrm",
  "wisvc",
  "wsearch",
  "wlidsvc",
  "wercplsupport",
  "wersvc",
  "wpcsvc",
];

const SYSTEM_LOC_MARKERS = [
  "\\microsoft\\windows\\",
  "\\system32\\",
  "\\syswow64\\",
  "\\windows\\servicing\\",
];

/**
 * True when the item is likely a Microsoft / Windows system entry.
 * Prefer path and explicit Microsoft tokens over bare "windows" substrings
 * (avoids hiding third-party tools that merely mention Windows).
 */
export function looksMicrosoft(it: ManageLike): boolean {
  const name = (it.name || "").toLowerCase();
  const detail = (it.detail || "").toLowerCase();
  const loc = (it.location || "").replace(/\//g, "\\").toLowerCase();
  const blob = `${name}\n${detail}`;

  if (MICROSOFT_TOKENS.some((t) => blob.includes(t))) return true;
  if (SYSTEM_LOC_MARKERS.some((m) => loc.includes(m))) return true;
  const leaf = name.split(/[\\/]/).pop() || name;
  if (SYSTEM_NAME_PREFIXES.some((p) => leaf.startsWith(p) && (leaf.length === p.length || !/[a-z]/.test(leaf[p.length] ?? "")))) {
    return true;
  }
  // Short pure system-style names like "W32Time" / "WpnUserService"
  if (/^w[a-z0-9]{2,14}$/.test(leaf) && !leaf.includes("web") && !leaf.includes("winget")) {
    // only when detail/path also looks system-ish or name is a known short token
    if (SYSTEM_NAME_PREFIXES.some((p) => leaf.startsWith(p))) return true;
  }
  return false;
}
