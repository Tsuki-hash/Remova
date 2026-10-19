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

export function cleanupFeedback(report: FullCleanupReport, L: Strings, channel?: string, prefix = "") {
  const message = report.aborted
    ? backendText(report.uninstall_message || report.errors[0] || "") || L.uninstallFail
    : `${prefix}${cleanupOutcomeText(report, L)}`;
  if (report.aborted || report.failed > 0) toast.error(message, { channel });
  else toast.success(message, { channel, ttl: 3000 });
  return message;
}
