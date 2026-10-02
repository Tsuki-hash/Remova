import type { FullCleanupReport } from "../types";
import type { Strings } from "../i18n";
import { backendText } from "./backendText";
import { toast } from "./toast";

export function cleanupFeedback(report: FullCleanupReport, L: Strings, channel?: string, prefix = "") {
  const message = report.aborted
    ? backendText(report.uninstall_message || report.errors[0] || "") || L.uninstallFail
    : `${prefix}${L.batchDetail(report.deleted, report.failed)}`;
  if (report.aborted || report.failed > 0) toast.error(message, { channel });
  else toast.success(message, { channel, ttl: 3000 });
  return message;
}
