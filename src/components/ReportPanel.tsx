import { api } from "../lib/api";
import type { VerifyRow } from "../lib/api";
import { runAiReportSummary } from "../lib/aiNarrative";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { cleanupProgress, gateReasonText } from "../lib/decision";
import type { CleanupReport, FullCleanupReport } from "../types";

type Props = {
  report: CleanupReport | FullCleanupReport;
  aiEnabled: boolean;
  aiReportBusy: boolean;
  aiReportNote: string | null;
  verifyRows: VerifyRow[] | null;
  onDismiss: () => void;
  onAiReportBusy: (v: boolean) => void;
  onAiReportNote: (v: string | null) => void;
};

export function ReportPanel({
  report,
  aiEnabled,
  aiReportBusy,
  aiReportNote,
  verifyRows,
  onDismiss,
  onAiReportBusy,
  onAiReportNote,
}: Props) {
  const L = t();
  return (
    <div
      style={{
        ...css.card,
        marginBottom: 10,
        padding: 12,
        fontSize: 13,
        background: "var(--surface-2)",
      }}
    >
      <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
        <strong>
          {"dry_run" in report && report.dry_run ? L.dryRunSummary : L.batchSummary}: {report.app_name}
        </strong>
        <button style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }} onClick={onDismiss}>
          ×
        </button>
      </div>
      {"backup_dir" in report &&
        (report.backup_dir ? (
          <div
            style={{
              marginTop: 8,
              padding: "6px 10px",
              borderRadius: 8,
              background: "var(--ok)",
              color: "#fff",
              fontSize: 12,
              fontWeight: 600,
              display: "inline-flex",
              alignItems: "center",
              gap: 8,
              width: "fit-content",
            }}
          >
            ✓ {L.safetyVaultBanner}
          </div>
        ) : !report.dry_run ? (
          <div style={{ marginTop: 8, fontSize: 12, color: "var(--muted)" }}>
            {L.noBackupThisRun}
          </div>
        ) : null)}
      {"deleted" in report && (
        <div
          style={{
            marginTop: 8,
            fontSize: 12.5,
            lineHeight: 1.5,
            color: "var(--muted)",
            borderLeft: "3px solid var(--accent)",
            paddingLeft: 10,
          }}
        >
          {L.reportNarrative(
            report.deleted,
            report.failed,
            report.skipped,
            Boolean("backup_dir" in report && report.backup_dir),
          )}
          {"delayed" in report && report.delayed ? (
            <span style={{ marginLeft: 6 }}>· {L.reportDelayedNote(report.delayed)}</span>
          ) : null}
          <span style={{ marginLeft: 6, fontSize: 10.5, fontWeight: 650 }}>
            · {aiReportNote ? L.conclusionSourceAi : L.conclusionSourceRule}
          </span>
          {"deleted" in report && "uninstall_ok" in report && (
            <div style={{ marginTop: 4, fontWeight: 600, color: "var(--fg)" }}>
              {L.reportFlowDone}
            </div>
          )}
          <div style={{ marginTop: 4, fontWeight: 500 }}>
            {report.failed > 0
              ? L.reportNextFailed
              : report.skipped > 0
                ? L.reportNextSkipped
                : L.reportNextOk}
          </div>
        </div>
      )}
      {"deleted" in report &&
        (() => {
          const p = cleanupProgress(report.deleted, report.failed, report.skipped);
          return (
            <div style={{ marginTop: 8, fontSize: 12.5 }}>
              <div style={{ fontWeight: 600, marginBottom: 4 }}>
                {p.complete
                  ? L.reportProgressComplete
                  : L.reportProgressLabel(p.handled, p.total, p.pct)}
              </div>
              <div
                style={{
                  height: 6,
                  borderRadius: 999,
                  background: "var(--surface)",
                  border: "1px solid var(--border)",
                  overflow: "hidden",
                }}
                aria-hidden
              >
                <div
                  style={{
                    width: `${p.pct}%`,
                    height: "100%",
                    background: p.complete ? "var(--ok)" : "var(--accent)",
                  }}
                />
              </div>
            </div>
          );
        })()}
      <div style={{ display: "flex", gap: 10, flexWrap: "wrap", marginTop: 10 }}>
        {("deleted_planned" in report
          ? [
              { label: L.dryRunPlanned, value: report.deleted_planned, tone: "var(--accent)" },
              { label: L.reportSkipped, value: report.skipped, tone: "var(--muted)" },
            ]
          : [
              { label: L.reportDeleted, value: report.deleted, tone: "var(--ok)" },
              { label: L.reportFailed, value: report.failed, tone: "var(--danger)" },
              { label: L.reportSkipped, value: report.skipped, tone: "var(--muted)" },
            ]
        ).map((s) => (
          <div
            key={s.label}
            style={{
              minWidth: 72,
              padding: "8px 12px",
              borderRadius: 10,
              background: "var(--surface)",
              border: "1px solid var(--border)",
              textAlign: "center" as const,
            }}
          >
            <div style={{ fontSize: 20, fontWeight: 700, color: s.tone, lineHeight: 1.1 }}>
              {s.value}
            </div>
            <div style={{ ...css.muted, fontSize: 11, marginTop: 2 }}>{s.label}</div>
          </div>
        ))}
        {"backup_dir" in report && report.backup_dir && (
          <button
            style={{ ...css.btnGhost, height: 36, alignSelf: "center" }}
            title={report.backup_dir}
            onClick={async () => {
              try {
                await api.openPath(report.backup_dir);
              } catch (e) {
                toast.error(L.errInvokeFailed(formatError(e)));
              }
            }}
          >
            {L.openBackupDir}
          </button>
        )}
        {aiEnabled && "deleted" in report && (
          <button
            style={{ ...css.btnGhost, height: 36, alignSelf: "center" }}
            disabled={aiReportBusy}
            onClick={async () => {
              onAiReportBusy(true);
              try {
                const note = await runAiReportSummary(report);
                onAiReportNote(note);
                if (!note) toast.error(L.aiFailed);
              } catch {
                toast.error(L.aiFailed);
              } finally {
                onAiReportBusy(false);
              }
            }}
          >
            {aiReportBusy ? L.aiReportBusy : L.aiReportSummary}
          </button>
        )}
      </div>
      {aiReportNote && (
        <div
          style={{
            marginTop: 10,
            padding: "8px 10px",
            borderRadius: 8,
            background: "var(--accent-soft)",
            fontSize: 12.5,
            lineHeight: 1.5,
          }}
        >
          ✦ {aiReportNote}
          <span style={{ color: "var(--muted)", marginLeft: 8 }}>· {L.aiDisclaimer}</span>
        </div>
      )}
      {verifyRows && verifyRows.length > 0 && (
        <div style={{ marginTop: 10 }}>
          <div style={{ fontWeight: 650, fontSize: 12.5, marginBottom: 4 }}>{L.verifyChecklist}</div>
          <div style={{ maxHeight: 120, overflow: "auto", fontSize: 12, color: "var(--muted)" }}>
            {verifyRows.slice(0, 40).map((v, i) => (
              <div key={`${v.path}-${i}`} className="ell" title={v.path}>
                <span style={{ color: v.still_there ? "var(--danger)" : "var(--ok)" }}>
                  {v.still_there ? "❌" : "✅"}
                </span>{" "}
                [{v.kind}] {v.path}
              </div>
            ))}
          </div>
        </div>
      )}
      {report.item_details.length > 0 && (
        <div style={{ maxHeight: 160, overflow: "auto", marginTop: 8, color: "var(--muted)" }}>
          {report.item_details.slice(0, 50).map((d, i) => (
            <div key={i} className="ell" title={d.path}>
              [{d.status}] {d.path}
              {d.message ? ` — ${gateReasonText(d.message, L)}` : ""}
            </div>
          ))}
          {report.item_details.length > 50 && <div>… +{report.item_details.length - 50}</div>}
        </div>
      )}
      {"uninstall_message" in report && report.uninstall_message && (
        <div style={{ marginTop: 6, color: "var(--muted)" }}>{report.uninstall_message}</div>
      )}
      {"restore_point_ok" in report && (
        <div style={{ marginTop: 4, color: "var(--muted)", fontSize: 12 }}>
          <span
            style={{
              color: report.restore_point_ok
                ? "var(--ok)"
                : report.restore_point_msg
                  ? "var(--warn)"
                  : "var(--muted)",
              fontWeight: 600,
            }}
          >
            {report.restore_point_ok
              ? L.restorePointOk
              : report.restore_point_msg
                ? L.restorePointFail
                : L.restorePointSkipped}
          </span>
          {report.restore_point_msg ? ` — ${report.restore_point_msg}` : ""}
        </div>
      )}
    </div>
  );
}
