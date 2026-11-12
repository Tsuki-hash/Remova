import type { FullCleanupReport } from "../types";

/**
 * Reduce every user-profile prefix to a placeholder. Applied recursively.
 *
 * Covers what the first version missed: the profile segment is matched
 * case-insensitively (`C:\users\Aaron`), UNC shares keep their server but lose
 * the profile name (`\\host\Users\Aaron`), and a relocated profile root
 * (`D:\Profiles\Aaron`) is recognised by its profile-shaped segment.
 */
export function redactUserProfiles(input: string): string {
  return input.replace(
    /((?:\\\\\?\\)?(?:[A-Za-z]:[\\/]+|\\\\[^\\/]+[\\/]+)(?:Users|Profiles|Home)[\\/]+)[^\\/]+/gi,
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

/** Export options: summary mode keeps counters only; redaction is on by default. */
export type AuditExportOptions = {
  summaryOnly?: boolean;
  /** Opt-in: export full user paths. Default (false) redacts to <user>. */
  includeUserPaths?: boolean;
};

function summarizeReport(report: FullCleanupReport) {
  return {
    dry_run: report.dry_run,
    backup_present: Boolean(report.backup_dir),
    backup_verified: report.backup_verified,
    uninstall_ok: report.uninstall_ok,
    deleted: report.deleted,
    failed: report.failed,
    skipped: report.skipped,
    delayed: report.delayed,
    reclaimed_kb: report.reclaimed_kb,
    kept_kb: report.kept_kb,
    delayed_kb: report.delayed_kb,
    aborted: report.aborted,
    reboot_pending: report.reboot_pending,
    reboot_gate_persisted: report.reboot_gate_persisted,
    restore_point_ok: report.restore_point_ok,
    error_count: report.errors.length,
  };
}

/** The exported shape: the report plus export metadata. */
export function buildAuditJson(
  report: FullCleanupReport,
  reviewRounds: number,
  reviewConverged: boolean | null,
  options: AuditExportOptions = {},
  reviewReason?: "insufficient" | "incomplete",
) {
  const summaryOnly = options.summaryOnly === true;
  const body = summaryOnly
    ? summarizeReport(report)
    : options.includeUserPaths ? report : redactDeep(report) as FullCleanupReport;
  return {
    exported_at: new Date().toISOString(),
    kind: "remova-audit-report",
    version: 1,
    /** Verify: SHA-256 of `JSON.stringify(JSON.parse(text).report)` (compact
     * JS serialization, key insertion order). */
    report_sha256_recipe: "SHA-256 of JSON.stringify(JSON.parse(text).report)",
    summary_only: summaryOnly,
    review_rounds: reviewRounds,
    review_converged: reviewConverged,
    review_unavailable_reason: reviewReason,
    report: body,
  };
}

const REVIEW_TEXT = {
  converged: "residue review converged (two complete rounds agree)",
  open: "residue review NOT converged",
  insufficient: "residue review has fewer than two complete rounds",
  incomplete: "a residue review round failed or was cancelled",
} as const;

/**
 * One review line that says *why* a verdict is unavailable. Passing `null`
 * without a reason was the bug: "fewer than two rounds" and "a round failed"
 * read the same, so a failed review looked like an unfinished one.
 */
function reviewLine(
  rounds: number,
  converged: boolean | null,
  reason?: "insufficient" | "incomplete",
): string {
  if (converged === null) {
    const text = reason === "incomplete" ? REVIEW_TEXT.incomplete : REVIEW_TEXT.insufficient;
    return `- Review: ${text} (${rounds} rounds)`;
  }
  return `- Review: ${converged ? REVIEW_TEXT.converged : REVIEW_TEXT.open} (${rounds} rounds)`;
}

/** Markdown rendering of the same evidence (per-item rows in full mode). */
export function buildAuditMarkdown(
  report: FullCleanupReport,
  reviewRounds: number,
  reviewConverged: boolean | null,
  options: AuditExportOptions = {},
  reviewReason?: "insufficient" | "incomplete",
): string {
  const summaryOnly = options.summaryOnly === true;
  const r = summaryOnly || options.includeUserPaths ? report : redactDeep(report) as FullCleanupReport;
  const backupStatus = !r.backup_dir ? "none" : r.backup_verified === true ? "verified" : "unverified";
  const backupLocation = !summaryOnly && r.backup_dir ? ` · ${r.backup_dir}` : "";
  const head = [
    summaryOnly ? "# Remova audit summary" : `# Remova audit — ${r.app_name}`,
    "",
    `- Exported: ${new Date().toISOString()}`,
    `- Uninstall: ${r.uninstall_ok ? "ok" : "failed"}${r.dry_run ? " (dry run)" : ""}`,
    `- Deleted: ${r.deleted} · Failed: ${r.failed} · Skipped: ${r.skipped}${r.delayed ? ` · Reboot-delayed: ${r.delayed}` : ""}`,
    `- Backup: ${backupStatus}${backupLocation}`,
    reviewLine(reviewRounds, reviewConverged, reviewReason),
    "",
  ];
  if (summaryOnly) {
    return head.join("\n");
  }
  const rows = (r.item_details || [])
    .map((d) => `| ${d.kind} | ${d.status} | ${d.path} | ${d.message} |`)
    .join("\n");
  return [
    ...head,
    "| Kind | Status | Path | Message |",
    "|---|---|---|---|",
    rows,
    "",
  ].join("\n");
}

/** SHA-256 over the exact report payload bytes — the export carries it so a
 * reviewer can re-hash the `report` field and verify nothing was altered. */
async function sha256Hex(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/** Build and trigger the download (same mechanism as the HTML report). */
export async function downloadAudit(
  report: FullCleanupReport,
  reviewRounds: number,
  reviewConverged: boolean | null,
  format: "json" | "md",
  options: AuditExportOptions = {},
  reviewReason?: "insufficient" | "incomplete",
) {
  const stem = options.summaryOnly ? "summary" :
    (report.app_name || "app").replace(/[\\/:*?"<>|]+/g, "_").replace(/\s+/g, "_");
  if (format === "md") {
    const md = buildAuditMarkdown(report, reviewRounds, reviewConverged, options, reviewReason);
    const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `remova-audit-${stem}.md`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
    return;
  }
  const payload = buildAuditJson(report, reviewRounds, reviewConverged, options, reviewReason);
  const json = JSON.stringify(
    { ...payload, report_sha256: await sha256Hex(JSON.stringify(payload.report)) },
    null,
    2,
  );
  const blob = new Blob([json], { type: "application/json;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `remova-audit-${stem}.json`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}
