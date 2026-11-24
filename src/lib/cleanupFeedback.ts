import type { FullCleanupReport } from "../types";
import type { Strings } from "../i18n";
import { backendText, isNamedStreamRefusal } from "./backendText";
import { gateReasonText } from "./decision";
import { toast } from "./toast";

export function cleanupItemReasonText(message: string, L: Strings): string {
  const known = gateReasonText(message, L);
  if (known !== L.backendDetailUnavailable) return known;
  if (/os error (32|33)\b|sharing violation|being used by another process|另一个进程|另一进程/i.test(message)) return L.reportFileBusy;
  if (/os error 5\b|access is denied|permission denied|拒绝访问/i.test(message)) return L.reportPermissionDenied;
  if (/os error (2|3)\b|not found|cannot find|找不到/i.test(message)) return L.reasonPathMissing;
  return known;
}

export function cleanupReportNextStep(report: FullCleanupReport, L: Strings) {
  const status = report.failed > 0 ? "failed" : report.skipped > 0 ? "skipped" : report.delayed ? "delayed" : null;
  if (!status && !report.aborted) return null;

  const items = status ? report.item_details.filter(item => item.status === status) : [];
  const target = items.length ? status : null;
  let message: string | null = report.aborted && !status ? L.reportNextCheck : null;
  if (status === "failed" || status === "skipped") {
    const reason = cleanupItemReasonText(items[0]?.message ?? "", L);
    // The per-item list may omit outcomes. Only name a common cause if every
    // reported item in this group is represented and has the same known cause.
    const allAgree = items.length === report[status] && reason && reason !== L.backendDetailUnavailable &&
      items.every(item => cleanupItemReasonText(item.message, L) === reason);
    if (allAgree) message = status === "failed" && reason === L.reportFileBusy
      ? L.reportNextBusy : L.reportNextReason(reason);
    else message = status === "failed" ? L.reportNextFailed : L.reportNextSkippedSummary;
  }
  return {
    message,
    restart: report.delayed ? L.reportNextDelayed : null,
    target,
    label: target === "failed" ? L.reportViewFailed : target === "skipped" ? L.reportViewSkipped
      : target === "delayed" ? L.reportViewDelayed : L.reportViewDetails,
  };
}

export function isLegacyHistoryWarning(error: string): boolean {
  return error.startsWith("cleanup history could not be saved:");
}

export function hasHistorySaveWarning(report: { history_save_failed?: boolean; errors: string[] }): boolean {
  return report.history_save_failed === true || report.errors.some(isLegacyHistoryWarning);
}

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

export function cleanupAbortText(report: Pick<FullCleanupReport, "uninstall_message" | "errors">, L: Strings): string {
  return backendText(report.errors.find(isNamedStreamRefusal) ?? (report.uninstall_message || report.errors[0] || "")) || L.uninstallFail;
}

/** May the report panel offer "rescan after uninstall"? A 1641/pending
 * report has `uninstall_ok` too: the uninstaller succeeded, only deletion was
 * postponed. */
export function shouldRescanAfterUninstall(report: {
  dry_run: boolean;
  uninstall_ok?: boolean;
}): boolean {
  return !report.dry_run && report.uninstall_ok === true;
}

export function cleanupFeedback(report: FullCleanupReport, L: Strings, channel?: string, prefix = "") {
  const message = report.aborted
    ? cleanupAbortText(report, L)
    : `${prefix}${cleanupOutcomeText(report, L)}`;
  if (report.aborted || report.failed > 0) toast.error(message, { channel });
  else if (cleanupAccomplishedSomething(report)) toast.success(message, { channel, ttl: 3000 });
  else toast.info(message, { channel });
  return message;
}
