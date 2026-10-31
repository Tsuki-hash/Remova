import type { FullCleanupReport } from "../types";

/**
 * M4E.7: user-triggered audit-evidence export. The JSON carries the cleanup
 * report plus a machine-checkable structure so a reviewer can re-derive the
 * verdict. Privacy: every `C:\Users\<name>\…` prefix is reduced to
 * `C:\Users\<user>\…` before anything leaves the webview, and the payload by
 * construction contains no `.npmrc` credentials (those never cross the IPC
 * boundary — see the CLI residue scanner).
 */

/** Reduce every user-profile prefix to a placeholder. Applied recursively. */
export function redactUserProfiles(input: string): string {
  // `C:\Users\Aaron\…`, `C:/Users/Aaron/…` and `\\?\C:\Users\Aaron\…`.
  return input.replace(
    /((?:\\\\\?\\)?[A-Za-z]:[\\/]+Users[\\/]+)[^\\/]+/g,
    "$1<user>",
  );
}

function redactDeep(value: unknown): unknown {
  if (typeof value === "string") return redactUserProfiles(value);
  if (Array.isArray(value)) return value.map(redactDeep);
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [key, v] of Object.entries(value)) {
      out[key] = redactDeep(v);
    }
    return out;
  }
  return value;
}

/** The exported shape: the report plus export metadata. */
export function buildAuditJson(
  report: FullCleanupReport,
  reviewRounds: number,
  reviewConverged: boolean | null,
) {
  return {
    exported_at: new Date().toISOString(),
    kind: "remova-audit-report",
    version: 1,
    review_rounds: reviewRounds,
    review_converged: reviewConverged,
    report: redactDeep(report),
  };
}

/** Build and trigger the download (same mechanism as the HTML report). */
export function exportAuditJson(
  report: FullCleanupReport,
  reviewRounds: number,
  reviewConverged: boolean | null,
) {
  const json = JSON.stringify(buildAuditJson(report, reviewRounds, reviewConverged), null, 2);
  const blob = new Blob([json], { type: "application/json;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `remova-audit-${(report.app_name || "app").replace(/[^\w.-]+/g, "_")}.json`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}
