/** Shared display helpers for app rows and reports. */
import { t } from "../i18n";

export function prettyPublisher(raw: string): string {
  const s = (raw || "").trim();
  if (!s || s === "—") return "—";
  if (/^CN=/i.test(s) || s.includes(", O=") || s.includes(",OU=")) {
    const cn = /CN=([^,]+)/i.exec(s)?.[1]?.trim();
    if (cn && !/^[0-9a-f-]{20,}$/i.test(cn) && cn.length <= 48) return cn;
    return "Signed package";
  }
  return s.length > 42 ? `${s.slice(0, 40)}…` : s;
}

export function prettyAppName(name: string, source: string): string {
  const n = (name || "").trim();
  if (!n) return "—";
  if (source === "Store" && /^[0-9a-f]{6,}(\.[0-9a-f]+)+$/i.test(n)) {
    return `Store app · ${n.slice(0, 8)}…`;
  }
  return n;
}

export function shortPath(p: string): string {
  const s = p || "";
  if (s.length <= 48) return s;
  return `${s.slice(0, 20)}…${s.slice(-20)}`;
}

/** Beginner-facing label for registry/store source codes. */
export function sourceLabel(source: string, L: { sourceHkcu: string; sourceHklm64: string; sourceHklm32: string; sourceStore: string }): string {
  switch (source) {
    case "HKCU":
      return L.sourceHkcu;
    case "HKLM64":
      return L.sourceHklm64;
    case "HKLM32":
      return L.sourceHklm32;
    case "Store":
      return L.sourceStore;
    default:
      return source;
  }
}

export type ErrorContext = "analyze" | "cleanup" | "elevate" | "invoke";

/** Stable backend error codes → user-facing text. */
export function formatError(e: unknown, ctx: ErrorContext = "invoke"): string {
  const L = t();
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  const elev = raw.match(/^elevate:(denied|cancelled|not_found|failed):(\d+)/);
  if (elev) {
    const kind = elev[1];
    const code = Number(elev[2]);
    if (kind === "denied") return L.errElevateDenied;
    if (kind === "cancelled") return L.errElevateCancelled;
    if (kind === "not_found") return L.errElevateNotFound;
    return L.errElevateFailed(code);
  }
  // manage:<kind>:<name>
  const mg = raw.trim().match(/^manage:(access_denied|open_failed|write_failed|protected):(.*)$/i);
  if (mg) {
    const kind = mg[1].toLowerCase();
    const name = mg[2].trim();
    if (kind === "access_denied") return L.errAccessAdminNamed(name);
    if (kind === "protected") return L.errServiceProtectedNamed(name);
    if (kind === "open_failed" || kind === "write_failed") {
      return L.errServiceKey(name);
    }
  }
  const detail = raw.replace(/^Error:\s*/i, "").trim() || raw;
  const low = detail.toLowerCase();
  if (low.includes("critical system service protected") || low.includes("critical service")) {
    return L.errServiceProtected;
  }
  // Legacy / untranslated leftovers
  if (
    low.includes("open service key failed") ||
    low.includes("write start failed") ||
    low.includes("access is denied") ||
    low.includes("拒绝访问")
  ) {
    const name =
      detail.match(/failed\s+(\S+)/i)?.[1] || detail.match(/for\s+(\S+)/i)?.[1] || "";
    return name ? L.errAccessAdminNamed(name) : L.errAccessAdmin;
  }
  if (ctx === "analyze") return L.errAnalyzeFailed(detail);
  if (ctx === "cleanup") return L.errCleanupFailed(detail);
  if (ctx === "elevate") return L.errElevateFailed(0);
  return L.errInvokeFailed(detail);
}

/** True when the backend said this is an elevation / ACL problem. */
export function isAccessDeniedError(e: unknown): boolean {
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  const low = raw.toLowerCase();
  return (
    low.includes("manage:access_denied") ||
    low.includes("open service key failed") ||
    low.includes("access is denied") ||
    low.includes("拒绝访问")
  );
}

export function escapeHtml(s: string): string {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
