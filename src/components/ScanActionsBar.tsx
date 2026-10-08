import type { ScanResult } from "../types";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { summarizeCleanupPlan } from "../lib/cleanupPlan";
import "./ScanActionsBar.css";

/** Scan action bar: back / title+totals / cleanup CTA / scope details / AI. */
export function ScanActionsBar({
  scan,
  scanning,
  dryRunning,
  residualFromUninstall,
  canRunOfficial = true,
  useOfficial,
  aiEnabled,
  aiBusy,
  selectedPaths,
  busy,
  onBack,
  onUseOfficial,
  onDryRun,
  onCleanup,
  onAiExplain,
  onOpenSettings,
  aiNote,
}: {
  scan: ScanResult;
  scanning: boolean;
  dryRunning: boolean;
  residualFromUninstall: boolean;
  canRunOfficial?: boolean;
  useOfficial: boolean;
  aiEnabled: boolean;
  aiBusy: boolean;
  selectedPaths: Set<string>;
  busy: boolean;
  onBack: () => void;
  onUseOfficial: (v: boolean) => void;
  onDryRun: () => void;
  onCleanup: () => void;
  onAiExplain: () => void;
  onOpenSettings?: () => void;
  aiNote?: string | null;
}) {
  const L = t();
  const plan = summarizeCleanupPlan(scan.items, selectedPaths);
  const runsOfficial = useOfficial && !residualFromUninstall && canRunOfficial;
  const confirmed = scan.items.filter((i) => i.confidence === "confirmed").length;
  const highRisk = scan.items.filter((i) => i.risk === "high").length;
  return (
    <>
      <section className="scan-actions" aria-label={L.leftoversTitle}>
        <div className="scan-actions-main">
          <button style={{ ...css.btnGhost, borderColor: "transparent", color: "var(--muted)" }} className="scan-actions-back" onClick={onBack}>
            ← {L.closePreview}
          </button>
          <div className="scan-actions-identity">
            <strong className="scan-actions-title" title={scan.app_name}>{scan.app_name}</strong>
            {scanning ? (
              <span className="scan-actions-caption" role="status">{L.analyzing}</span>
            ) : (
              <span className="scan-actions-caption">
                {L.scanTotals(scan.items.length, confirmed)}
              </span>
            )}
          </div>
          <div className="scan-actions-primary">
            {!scanning && plan.count > 0 && (
              <div className="scan-actions-estimate" title={L.cleanupPlanEstimate}>
                <span>{L.colSelected} {plan.count}</span>
                <strong>{L.conclusionSpace(formatSize(plan.knownKb))}</strong>
              </div>
            )}
            <button
              style={{ ...css.btn, height: 38, background: "var(--danger-text)", color: "var(--surface)" }}
              className="scan-actions-cleanup"
              disabled={dryRunning || plan.count === 0 || busy}
              onClick={onCleanup}
              title={runsOfficial ? L.officialFirstHint : L.cleanupSelectedHint}
            >
              {runsOfficial ? L.uninstallAndCleanup : `${L.cleanup} (${plan.count})`}
            </button>
          </div>
        </div>
        {scanning && <div className="remova-progress" aria-hidden />}
        <div className="scan-actions-secondary">
          {!residualFromUninstall && canRunOfficial && (
            <span className="scan-actions-caption" style={{ color: useOfficial ? undefined : "var(--warn-ink)" }}>
              {useOfficial ? L.officialFirstHint : L.skipOfficialHint}
            </span>
          )}
          {!scanning && plan.count === 0 && (
            <span className="scan-actions-caption">{L.footerSelected(0)}</span>
          )}
          {!scanning && plan.unknown > 0 && (
            <span className="scan-actions-caption">{L.cleanupPlanUnknown(plan.unknown)}</span>
          )}
          {!scanning && plan.review > 0 && (
            <span
              style={{
                ...css.muted,
                fontSize: 12,
                color: plan.review > 0 ? "var(--warn-ink)" : undefined,
              }}
              title={L.cleanupPlanReview(plan.review)}
            >
              {L.cleanupPlanReview(plan.review)}
            </span>
          )}
          {!scanning && highRisk > 0 && (
            <span style={{ color: "var(--danger-text)", fontSize: 12 }}>
              {L.riskHigh} {highRisk}
            </span>
          )}
          <details className="scan-actions-details">
            <summary style={{ cursor: "pointer" }}>{L.cleanupPlanDetails}</summary>
            <div className="scan-actions-explanation">
              <span>{L.cleanupPlanOther(plan.registry, plan.path)}</span>
              <span>{L.cleanupPlanBackup}</span>
              <span>{L.cleanupPlanEstimate}</span>
              {!residualFromUninstall && canRunOfficial && (
                <details>
                  <summary style={{ cursor: "pointer" }}>{L.cleanupAdvanced}</summary>
                  <label className="scan-actions-official" title={L.skipOfficialHint}>
                    <input
                      type="checkbox"
                      checked={!useOfficial}
                      disabled={busy || dryRunning || scanning}
                      onChange={(e) => onUseOfficial(!e.target.checked)}
                      style={{ accentColor: "var(--accent)" }}
                    />
                    {L.skipOfficial}
                  </label>
                  <div>{L.skipOfficialHint}</div>
                </details>
              )}
              <span id="cleanup-plan-check-hint">{L.dryRunHint}</span>
              <button
                style={{ ...css.btnGhost, justifySelf: "start" }}
                disabled={busy || dryRunning || plan.count === 0}
                onClick={onDryRun}
                aria-describedby="cleanup-plan-check-hint"
              >
                {L.dryRun}
              </button>
            </div>
          </details>
          {aiEnabled ? (
            <button
              style={{ ...css.btnSm, borderColor: "transparent", color: "var(--accent-text)" }}
              className="scan-actions-ai"
              disabled={aiBusy || scan.items.length === 0}
              title={L.aiSettingsHint}
              onClick={onAiExplain}
            >
              {aiBusy ? L.aiExplaining : L.conclusionRefreshAi}
            </button>
          ) : (
            onOpenSettings && (
              <button
                style={{ ...css.btnSm, borderColor: "transparent", color: "var(--accent-text)" }}
                className="scan-actions-ai"
                onClick={onOpenSettings}
                title={L.aiSettingsHint}
              >
                {L.conclusionEnableAi}
              </button>
            )
          )}
        </div>
      </section>
      {aiNote && (
        <div
          style={{
            marginBottom: 10,
            padding: "8px 12px",
            border: "1px solid var(--border)",
            borderRadius: 8,
            background: "var(--surface-2)",
            fontSize: 12,
            lineHeight: 1.5,
            color: "var(--muted)",
          }}
        >
          {aiNote} <span>· {L.aiDisclaimer}</span>
        </div>
      )}
    </>
  );
}
