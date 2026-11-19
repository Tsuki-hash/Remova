import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { BackButton } from "./ui/BackButton";
import { Deco } from "./ui/Glyph";
import type { VerifyRow } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { cleanupProgress } from "../lib/decision";
import { CleanupResultDetails } from "./CleanupResultDetails";
import { DeepRegistryCheck } from "./DeepRegistryCheck";
import { requestBackupSession } from "../lib/backupSessionNavigation";
import { backendText } from "../lib/backendText";
import { cleanupAbortText, hasHistorySaveWarning } from "../lib/cleanupFeedback";
import { downloadAudit } from "../lib/exportAuditReport";
import { reviewSessionFor, reviewVerdictFor } from "../lib/rescanRounds";
import { requestConfirmEx } from "../lib/confirm";
import { uninstallRescanText, type UninstallRescan } from "../lib/uninstallReview";
import type { CleanupReport, FullCleanupReport } from "../types";

type Props = {
  report: CleanupReport | FullCleanupReport;
  aiEnabled: boolean;
  aiReportBusy: boolean;
  aiReportNote: string | null;
  verifyRows: VerifyRow[] | null;
  residualScan?: UninstallRescan;
  residualRestart?: "required" | "before_verification";
  /** re-analyze the same app without preselecting anything. */
  onRescan?: () => void;
  rescanBusy?: boolean;
  /** explicit read-only deep registry check for this app. */
  deepCheckApp?: { name: string } | null;
  /** install directory bounding the "end process and retry" action. */
  appInstallLocation?: string;
  onDismiss: () => void;
  onRegenerate: () => void;
};

export function ReportPanel({
  report,
  aiEnabled,
  aiReportBusy,
  aiReportNote,
  verifyRows,
  residualScan,
  residualRestart,
  onRescan,
  rescanBusy,
  deepCheckApp,
  appInstallLocation,
  onDismiss,
  onRegenerate,
}: Props) {
  const L = t();
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    panel.current?.scrollIntoView?.({ block: "start", behavior: report.dry_run || reducedMotion ? "auto" : "smooth" });
  }, [report]);
  const [auditBusy, setAuditBusy] = useState(false);
  const exportAudit = async (format: "json" | "md", summaryOnly: boolean) => {
    if (!("deleted" in report) || report.dry_run) return;
    const session = reviewSessionFor(report);
    const rounds = session?.rounds.length ?? 0;
    const verdict = reviewVerdictFor(session);
    // "Fewer than two rounds" and "a round failed" are different answers; the
    // export must say which one applies instead of one generic unknown.
    const verdictText =
      verdict.state === "converged"
        ? L.auditConvergedYes
        : verdict.state === "diverged"
          ? L.auditConvergedNo
          : verdict.state === "insufficient_rounds"
            ? L.auditConvergedInsufficient(verdict.rounds)
            : L.auditConvergedIncomplete;
    const { ok, checked } = await requestConfirmEx({
      title: L.auditExportTitle,
      message: L.auditExportPreview(
        report.app_name,
        report.deleted,
        report.failed,
        rounds,
        verdictText,
        summaryOnly,
      ),
      confirmLabel: L.auditExportGo,
      checkbox: summaryOnly ? undefined : { label: L.auditExportIncludePaths, defaultChecked: false },
    });
    if (!ok) return;
    setAuditBusy(true);
    try {
      let profileRoot: string | undefined;
      if (!summaryOnly && !checked) {
        profileRoot = (await api.auditProfileRoot()) ?? undefined;
        if (!profileRoot) throw new Error(L.auditExportProfileUnavailable);
      }
      await downloadAudit(
        report,
        rounds,
        verdict.state === "converged" ? true : verdict.state === "diverged" ? false : null,
        format,
        { summaryOnly, includeUserPaths: checked, profileRoot },
        verdict.state === "incomplete_round"
          ? "incomplete"
          : verdict.state === "insufficient_rounds"
            ? "insufficient"
            : undefined,
      );
    } catch (e) {
      toast.error(formatError(e));
    } finally {
      setAuditBusy(false);
    }
  };

  const backupDir = "backup_dir" in report && !report.dry_run ? report.backup_dir : "";
  const backupSession = backupDir && "backup_verified" in report && report.backup_verified === true
    ? backupDir.split(/[\\/]/).filter(Boolean).at(-1) : undefined;
  const planned = report.dry_run ? ("deleted_planned" in report ? report.deleted_planned : report.deleted) : null;
  const result = !report.dry_run && "deleted" in report ? report : null;
  return (
    <div
      ref={panel}
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
        <BackButton style={{ marginLeft: "auto" }} onClick={onDismiss} />
      </div>
      {planned !== null && (
        <p role="status" style={{ display: "flex", flexWrap: "wrap", gap: "4px 16px", margin: "8px 0" }}>
          <strong>{L.dryRunPlanned} {planned}</strong>
          <span style={css.muted}>{L.reportSkipped} {report.skipped}</span>
        </p>
      )}
      {result && (
        <div role="group" aria-label={L.cleanupResultCounts} style={{ display: "flex", gap: 8, flexWrap: "wrap", marginTop: 8 }}>
          {[
            { label: L.reportDeleted, value: result.deleted, tone: "var(--ok-ink)" },
            { label: L.reportSkipped, value: result.skipped, tone: "var(--muted)" },
            { label: L.reportFailed, value: result.failed, tone: "var(--danger-text)" },
            { label: L.reportPendingReboot, value: result.delayed ?? 0, tone: "var(--warn-ink)" },
          ].map(({ label, value, tone }) => (
            <div key={label} style={{
              minWidth: 64, padding: "6px 10px", border: "1px solid var(--border)",
              borderRadius: 8, background: "var(--surface)",
            }}>
              <strong style={{ fontSize: 18, color: tone }}>{value}</strong>
              <div style={css.muted}>{label}</div>
            </div>
          ))}
        </div>
      )}
      {"uninstall_ok" in report && !report.dry_run && !report.aborted && (report.uninstall_ok || residualScan) && (
        <div role="status" style={{ marginTop: 8, lineHeight: 1.6 }}>
          <strong>{L.uninstallOk}</strong>
          <div style={{ color: "var(--muted)" }}>{uninstallRescanText(L, residualScan, true)}</div>
        </div>
      )}
      {hasHistorySaveWarning(report) && (
        <div role="status" style={{ marginTop: 8, color: "var(--warn-ink)" }}>
          {L.cleanupHistorySaveFailed}
        </div>
      )}
      {residualRestart && (
        <div role="status" style={{ marginTop: 8, color: "var(--warn-ink)" }}>
          {residualRestart === "required" ? L.backendRebootRequired : L.backendUninstallRebootBeforeVerify}
        </div>
      )}
      {!report.dry_run && "deleted" in report &&
      (report.reclaimed_kb || report.kept_kb || report.delayed_kb) ? (
        <div style={{ ...css.muted, marginTop: 6, fontSize: 12 }}>
          {L.reportReclaimedKept(
            formatSize(report.reclaimed_kb ?? 0),
            formatSize(report.kept_kb ?? 0),
          )}
          {report.delayed_kb
            ? ` · ${L.reportPendingKb(formatSize(report.delayed_kb))}`
            : null}
        </div>
      ) : null}
      {(result || onRescan) && (
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginTop: 8 }}>
          {backupSession ? (
            <>
              <span style={{ alignSelf: "center", color: "var(--ok-ink)" }}><Deco ch="✓" /> {L.safetyVaultBanner}</span>
              <button type="button" style={css.btnGhost}
                aria-label={`${L.restorePreviewTitle}: ${backupSession}`} onClick={() => requestBackupSession(backupSession)}>
                {L.restorePreviewTitle}
              </button>
            </>
          ) : result && (
            <span role="note" style={{ alignSelf: "center", color: "var(--warn-ink)" }}>
              {backupDir ? L.unverifiedBackupThisRun : L.noBackupThisRun}
            </span>
          )}
          {onRescan && (
            <button type="button" style={css.btnGhost} disabled={rescanBusy} onClick={onRescan}>
              {rescanBusy ? L.analyzing : L.reportRescanApp}
            </button>
          )}
        </div>
      )}

      <details open={Boolean(result && (result.aborted || result.failed || result.skipped))} style={{ marginTop: 8 }}>
        <summary style={{ cursor: "pointer", fontWeight: 600 }}>{L.cleanupReportDetails}</summary>
        <div style={{ maxHeight: 360, overflow: "auto" }}>
      {!report.dry_run && "deleted" in report && (
        <div
          style={{
            marginTop: 8,
            fontSize: 13,
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
            Boolean(backupSession),
          )}
          {"delayed" in report && report.delayed ? (
            <span style={{ marginLeft: 6 }}>· {L.reportDelayedNote(report.delayed)}</span>
          ) : null}
          <span style={{ marginLeft: 6, fontSize: 10.5, fontWeight: 650 }}>
            · {aiReportNote ? L.conclusionSourceAi : L.conclusionSourceRule}
          </span>
          {!report.aborted && report.uninstall_ok &&
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
                : report.aborted
                  ? L.reportNextCheck
                  : report.delayed
                    ? L.reportNextDelayed
                  : backupSession
                    ? L.reportNextOk
                    : L.reportNextNoBackup}
          </div>
        </div>
      )}
      {!report.dry_run && "deleted" in report &&
        (() => {
          const p = cleanupProgress(report.deleted, report.failed + report.skipped + (report.delayed ?? 0), 0);
          return (
            <div style={{ marginTop: 8, fontSize: 13 }}>
              <div style={{ fontWeight: 600, marginBottom: 4 }}>
                {p.complete && !report.aborted
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
                    background: p.complete && !report.aborted ? "var(--ok)" : "var(--accent)",
                  }}
                />
              </div>
            </div>
          );
        })()}
      <div style={{ display: "flex", gap: 10, flexWrap: "wrap", marginTop: 10 }}>
        {backupDir && (
          <button
            style={{ ...css.btnGhost, height: 36, alignSelf: "center" }}
            title={backupDir}
            onClick={() => {
              void (async () => {
                try {
                  await api.openPath(backupDir);
                } catch (e) {
                  toast.error(formatError(e));
                }
              })();
            }}
          >
            {L.openBackupDir}
          </button>
        )}
        {aiEnabled && !report.dry_run && "deleted" in report && (
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
            fontSize: 13,
            lineHeight: 1.5,
          }}
        >
          <Deco ch="✦" /> {aiReportNote}
          <span style={{ color: "var(--muted)", marginLeft: 8 }}>· {L.aiDisclaimer}</span>
        </div>
      )}
      {verifyRows && verifyRows.length > 0 && (
        <div style={{ marginTop: 10 }}>
          <div style={{ fontWeight: 650, fontSize: 13, marginBottom: 4 }}>{L.verifyChecklist}</div>
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
      {deepCheckApp && <DeepRegistryCheck key={deepCheckApp.name} appName={deepCheckApp.name} />}
      {!!report.item_details.length && (
        <CleanupResultDetails
          key={JSON.stringify(report.item_details)}
          items={report.item_details}
          lockRoot={appInstallLocation}
          onRescan={onRescan}
          expandSkipped={Boolean(result?.skipped)}
        />
      )}
      {"uninstall_message" in report && report.uninstall_message && (
        <div style={{ marginTop: 6, color: "var(--muted)" }}>
          {"aborted" in report && report.aborted ? cleanupAbortText(report, L) : backendText(report.uninstall_message)}
        </div>
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
      </details>
      {result && (
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginTop: 8 }}>
          <button type="button" style={css.btnGhost} disabled={auditBusy} onClick={() => void exportAudit("json", false)}>
            {L.auditExportJson}
          </button>
          <button type="button" style={css.btnGhost} disabled={auditBusy} onClick={() => void exportAudit("md", false)}>
            {L.auditExportMd}
          </button>
          <button type="button" style={css.btnGhost} disabled={auditBusy} onClick={() => void exportAudit("json", true)}>
            {L.auditExportSummary}
          </button>
        </div>
      )}
    </div>
  );
}
