import type { FullCleanupReport } from "../types";
import type { Strings } from "../i18n";
import { backendText } from "./backendText";
import { toast } from "./toast";

/**
 * Headline for a finished cleanup. A 3–7 second toast that only names
 * deleted/failed hides the skipped and reboot-delayed counts, so all four
 * outcomes are stated; the per-item detail lives in `CleanupResultDetails`,
 * which the panel keeps on screen after the toast is gone.
 */
export function cleanupOutcomeText(report: FullCleanupReport, L: Strings): string {
  return L.cleanupOutcome(report.deleted, report.failed, report.skipped, report.delayed ?? 0);
}

/**
 * Did the cleanup actually do what the user asked?
 *
 * `failed === 0` is not enough. A user can hand-pick rows from the "suggest"
 * bucket — high-risk, shared-component and user-data items that
 * `defaultSelectable` never preselects — and the server-side gate refuses every
 * one of them: `deleted: 0, failed: 0, skipped: N, aborted: false`. Reporting
 * that as a green "ok 0 · failed 0" told the user their selection had been
 * carried out. `uninstall_ok` counts on its own: the official uninstaller
 * succeeding with no leftover files to delete is a real outcome.
 */
export function cleanupAccomplishedSomething(report: FullCleanupReport): boolean {
  if (report.aborted || report.failed > 0) return false;
  return report.deleted > 0 || (report.delayed ?? 0) > 0 || report.uninstall_ok;
}

/**
 * Why nothing was cleaned, stated so the user can act on it. A non-empty
 * `skipped` means the keep rules refused the selection; otherwise there was
 * simply nothing to delete.
 */
export function cleanupNothingText(report: FullCleanupReport, L: Strings): string {
  return L.cleanupNothingRemoved(report.skipped);
}

export function cleanupFeedback(report: FullCleanupReport, L: Strings, channel?: string, prefix = "") {
  const message = report.aborted
    ? backendText(report.uninstall_message || report.errors[0] || "") || L.uninstallFail
    : `${prefix}${cleanupOutcomeText(report, L)}`;
  if (report.aborted || report.failed > 0) toast.error(message, { channel });
  else toast.success(message, { channel, ttl: 3000 });
  return message;
}
