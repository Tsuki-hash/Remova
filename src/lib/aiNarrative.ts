/** Shared AI report narrative helpers (App auto path + ReportPanel manual path). */
import { api } from "./api";
import type { CleanupReport, FullCleanupReport } from "../types";

export function buildReportAiRequest(report: FullCleanupReport | CleanupReport) {
  const details = "item_details" in report ? report.item_details || [] : [];
  const topFailed = details
    .filter((d) => d.status && d.status.toLowerCase().includes("fail"))
    .slice(0, 5)
    .map((d) => d.path);
  return {
    appName: report.app_name,
    deleted: "deleted" in report ? report.deleted : 0,
    failed: "failed" in report ? report.failed : 0,
    skipped: report.skipped,
    aborted: "aborted" in report ? report.aborted : false,
    backupDir: "backup_dir" in report ? report.backup_dir || "" : "",
    restorePointOk: "restore_point_ok" in report ? report.restore_point_ok : true,
    topFailed,
  };
}

export async function runAiReportSummary(
  report: FullCleanupReport | CleanupReport,
): Promise<string | null> {
  return api.aiSummarizeReport(buildReportAiRequest(report));
}
