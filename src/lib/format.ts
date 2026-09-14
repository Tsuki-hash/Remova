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

export type ErrorContext = "analyze" | "cleanup" | "elevate" | "invoke";

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
  const detail = raw.replace(/^Error:\s*/i, "").trim() || raw;
  if (ctx === "analyze") return L.errAnalyzeFailed(detail);
  if (ctx === "cleanup") return L.errCleanupFailed(detail);
  if (ctx === "elevate") return L.errElevateFailed(0);
  return L.errInvokeFailed(detail);
}

export function escapeHtml(s: string): string {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
