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

/** Export options: summary mode keeps counters only; redaction is on by default. */
export type AuditExportOptions = {
  summaryOnly?: boolean;
  /** Opt-in: export full user paths. Default (false) redacts to <user>. */
  includeUserPaths?: boolean;
};

/** The exported shape: the report plus export metadata. */
export function buildAuditJson(
  report: FullCleanupReport,
  reviewRounds: number,
  reviewConverged: boolean | null,
  options: AuditExportOptions = {},
) {
  const body = options.includeUserPaths ? report : (redactDeep(report) as FullCleanupReport);
  const base = {
    exported_at: new Date().toISOString(),
    kind: "remova-audit-report",
    version: 1,
    summary_only: options.summaryOnly === true,
    review_rounds: reviewRounds,
    review_converged: reviewConverged,
    report: body,
  };
  if (options.summaryOnly !== true) return base;
  // Summary mode: counters and verdicts without per-item paths.
  const { item_details: _details, ...rest } = base.report;
  return { ...base, report: rest };
}

const REVIEW_TEXT = {
  converged: "residue review converged (two complete rounds agree)",
  open: "residue review NOT converged",
  unknown: "residue review has fewer than two complete rounds",
} as const;

function reviewLine(rounds: number, converged: boolean | null): string {
  if (rounds < 2 || converged === null) return `- Review: ${REVIEW_TEXT.unknown}`;
  return `- Review: ${converged ? REVIEW_TEXT.converged : REVIEW_TEXT.open} (${rounds} rounds)`;
}

/** Markdown rendering of the same evidence (per-item rows in full mode). */
export function buildAuditMarkdown(
  report: FullCleanupReport,
  reviewRounds: number,
  reviewConverged: boolean | null,
  options: AuditExportOptions = {},
): string {
  const redact = options.includeUserPaths !== true;
  const r = redact ? (redactDeep(report) as FullCleanupReport) : report;
  const head = [
    `# Remova audit — ${r.app_name}`,
    "",
    `- Exported: ${new Date().toISOString()}`,
    `- Uninstall: ${r.uninstall_ok ? "ok" : "failed"}${r.dry_run ? " (dry run)" : ""}`,
    `- Deleted: ${r.deleted} · Failed: ${r.failed} · Skipped: ${r.skipped}${r.delayed ? ` · Reboot-delayed: ${r.delayed}` : ""}`,
    `- Backup: ${r.backup_dir || "(none)"}`,
    reviewLine(reviewRounds, reviewConverged),
    "",
  ];
  if (options.summaryOnly === true) {
    return [...head, "_Summary mode: per-item paths omitted._"].join("\n");
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

/** Build and trigger the download (same mechanism as the HTML report). */
export function downloadAudit(
  report: FullCleanupReport,
  reviewRounds: number,
  reviewConverged: boolean | null,
  format: "json" | "md",
  options: AuditExportOptions = {},
) {
  const stem = (report.app_name || "app").replace(/[^\w.-]+/g, "_");
  if (format === "md") {
    const md = buildAuditMarkdown(report, reviewRounds, reviewConverged, options);
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
  const json = JSON.stringify(
    buildAuditJson(report, reviewRounds, reviewConverged, options),
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
