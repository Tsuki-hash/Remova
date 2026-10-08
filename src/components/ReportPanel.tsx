import { api } from "../lib/api";
import { CloseGlyph, Deco } from "./ui/Glyph";
import type { VerifyRow } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { cleanupProgress } from "../lib/decision";
import { CleanupResultDetails } from "./CleanupResultDetails";
import { requestBackupSession } from "../lib/backupSessionNavigation";
import { backendText } from "../lib/backendText";
import type { CleanupReport, FullCleanupReport } from "../types";

type Props = {
  report: CleanupReport | FullCleanupReport;
  aiEnabled: boolean;
  aiReportBusy: boolean;
  aiReportNote: string | null;
  verifyRows: VerifyRow[] | null;
  onDismiss: () => void;
  onRegenerate: () => void;
};

export function ReportPanel({
  report,
  aiEnabled,
  aiReportBusy,
  aiReportNote,
  verifyRows,
  onDismiss,
  onRegenerate,
}: Props) {
  const L = t();
  const backupSession = "backup_dir" in report && !report.dry_run && !report.aborted && report.backup_dir
    ? report.backup_dir.split(/[\\/]/).filter(Boolean).at(-1) : undefined;
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
        <button style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }} onClick={onDismiss} aria-label={L.panelClose}>
          <CloseGlyph />
        </button>
      </div>
      {"backup_dir" in report &&
        (backupSession ? (
          <div
            style={{
              marginTop: 8,
              padding: "6px 10px",
              borderRadius: 8,
              background: "var(--ok)",
 // Dark ink keeps AA contrast on the ok token in both themes
 // (white on the dark-theme green falls below 4.5:1).
              color: "#0B1220",
              fontSize: 12,
              fontWeight: 600,
              display: "inline-flex",
              alignItems: "center",
              gap: 8,
              width: "fit-content",
            }}
          >
            <Deco ch="✓" /> {L.safetyVaultBanner}
          </div>
        ) : !report.dry_run && !report.backup_dir ? (
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
          {"uninstall_ok" in report && !report.dry_run && !report.aborted && report.uninstall_ok &&
            report.deleted > 0 && !report.failed && !report.skipped && !report.delayed && (
            <div style={{ marginTop: 4, fontWeight: 600, color: "var(--fg)" }}>
              {L.reportFlowDone}
            </div>
          )}
          <div style={{ marginTop: 4, fontWeight: 500 }}>
            {report.failed > 0
              ? L.reportNextFailed
              : report.skipped > 0
                ? L.reportNextSkipped
                : ("dry_run" in report && report.dry_run) || ("aborted" in report && report.aborted)
                  ? L.reportNextCheck
                  : report.delayed
                    ? L.reportNextDelayed
                  : "backup_dir" in report && report.backup_dir
                    ? L.reportNextOk
                    : L.reportNextNoBackup}
          </div>
        </div>
      )}
      {"deleted" in report &&
        (() => {
          const p = cleanupProgress(report.deleted, report.failed + report.skipped + (report.delayed ?? 0), 0);
          return (
            <div style={{ marginTop: 8, fontSize: 12.5 }}>
              <div style={{ fontWeight: 600, marginBottom: 4 }}>
                {p.complete && !report.dry_run && !report.aborted
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
                    background: p.complete && !report.dry_run && !report.aborted ? "var(--ok)" : "var(--accent)",
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
              { label: L.reportPendingReboot, value: report.delayed ?? 0, tone: "var(--warn-ink)" },
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
            onClick={() => {
              void (async () => {
                try {
                  await api.openPath(report.backup_dir);
                } catch (e) {
                  toast.error(formatError(e));
                }
              })();
            }}
          >
            {L.openBackupDir}
          </button>
        )}
        {backupSession && <button style={{ ...css.btnGhost, height: 36, alignSelf: "center" }}
          aria-label={`${L.restorePreviewTitle}: ${backupSession}`} onClick={() => requestBackupSession(backupSession)}>
          {L.restorePreviewTitle}
        </button>}
        {aiEnabled && "deleted" in report && (
          <button
            style={{ ...css.btnGhost, height: 36, alignSelf: "center" }}
            disabled={aiReportBusy}
 // regenerate goes through the hook's seq-guarded
 // runAiReport — the old inline path raced the auto effect.
            onClick={onRegenerate}
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
          <Deco ch="✦" /> {aiReportNote}
          <span style={{ color: "var(--muted)", marginLeft: 8 }}>· {L.aiDisclaimer}</span>
        </div>
      )}
      {verifyRows && verifyRows.length > 0 && (
        <div style={{ marginTop: 10 }}>
          <div style={{ fontWeight: 650, fontSize: 12.5, marginBottom: 4 }}>{L.verifyChecklist}</div>
          <div style={{ maxHeight: 120, overflow: "auto", fontSize: 12, color: "var(--muted)" }}>
            {verifyRows.map((v, i) => (
              <div key={`${v.path}-${i}`} className="ell" title={v.path}>
                <span
                  role="img"
                  aria-label={v.error ? L.verifyUnknown : v.still_there ? L.verifyStillPresent : L.verifyRemoved}
                  style={{
                    color: v.error ? "var(--warn-ink)" : v.still_there ? "var(--danger-text)" : "var(--ok-ink)",
                  }}
                >
                  <Deco ch={v.error ? "ⓘ" : v.still_there ? "×" : "✓"} />
                </span>{" "}
                [{L.kindLabel(v.kind)}] {v.path}
                {v.error ? ` — ${L.verifyUnknown}` : ""}
              </div>
            ))}
          </div>
        </div>
      )}
      {!!report.item_details.length && <CleanupResultDetails key={JSON.stringify(report.item_details)} items={report.item_details} />}
      {"uninstall_message" in report && report.uninstall_message && (
        <div style={{ marginTop: 6, color: "var(--muted)" }}>{backendText(report.uninstall_message)}</div>
      )}
      {"restore_point_ok" in report && (
        <div style={{ marginTop: 4, color: "var(--muted)", fontSize: 12 }}>
          <span
            style={{
              color: report.restore_point_ok
                ? "var(--ok-ink)"
                : report.restore_point_msg
                  ? "var(--warn-ink)"
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
          {report.restore_point_msg ? ` — ${backendText(report.restore_point_msg)}` : ""}
        </div>
      )}
      <details style={{ marginTop: 8, fontSize: 12 }}>
        <summary>{L.backendTechnicalDetails}</summary>
        <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
          {[report.uninstall_message, "restore_point_msg" in report ? report.restore_point_msg : "",
            ...report.item_details.map(d => `${d.path}: ${d.message}`), ...report.errors].filter(Boolean).join("\n")}
        </pre>
      </details>
    </div>
  );
}
