import type { ScanResult } from "../types";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { summarizeCleanupPlan } from "../lib/cleanupPlan";

/** Scan action bar: back / title+totals / dry-run / cleanup CTA / scope details / AI. */
export function ScanActionsBar({
  scan,
  scanning,
  dryRunning,
  residualFromUninstall,
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
  const confirmed = scan.items.filter((i) => i.confidence === "confirmed").length;
  const highRisk = scan.items.filter((i) => i.risk === "high").length;
  return (
    <>
      <div
        style={{
          ...css.toolbar,
          marginBottom: 10,
          padding: "8px 12px",
          background: "var(--surface-2)",
          border: "1px solid var(--border)",
          borderRadius: 8,
        }}
      >
        <button style={css.btnGhost} onClick={onBack}>
          ← {L.closePreview}
        </button>
        <strong style={{ fontSize: 13, fontWeight: 600 }}>{scan.app_name}</strong>
        {scanning ? (
          <span style={{ ...css.muted }}>{L.analyzing}</span>
        ) : (
          <span style={{ ...css.muted }}>
            {L.leftoversTitle}: {L.scanTotals(scan.items.length, confirmed)}
          </span>
        )}
        {!residualFromUninstall && (
          <label
            style={{
              fontSize: 12.5,
              display: "flex",
              alignItems: "center",
              gap: 6,
              color: "var(--muted)",
            }}
            title={L.batchOfficialHint}
          >
            <input
              type="checkbox"
              checked={useOfficial}
              onChange={(e) => onUseOfficial(e.target.checked)}
              style={{ accentColor: "var(--accent)" }}
            />
            {L.useOfficial}
          </label>
        )}
        {scanning && <div className="remova-progress" style={{ marginTop: 8, flexShrink: 0 }} aria-hidden />}
        <button style={css.btnGhost} disabled={busy || dryRunning || plan.count === 0} onClick={onDryRun}>
          {L.dryRun}
        </button>
        <button
          style={{ ...css.btn, background: "var(--danger-text)", color: "var(--surface)" }}
          disabled={dryRunning || plan.count === 0 || busy}
          onClick={onCleanup}
          title={L.dangerScopeHint}
        >
          {L.cleanup} ({plan.count})
        </button>
        {!scanning && plan.count > 0 && (
          <span style={{ ...css.muted, fontSize: 12 }} title={L.cleanupPlanEstimate}>
            {L.conclusionSpace(formatSize(plan.knownKb))}
            {plan.unknown > 0 ? ` · ${L.cleanupPlanUnknown(plan.unknown)}` : ""}
          </span>
        )}
        {!scanning && plan.count > 0 && (
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
        <details style={{ fontSize: 12, color: "var(--muted)", marginLeft: "auto" }}>
          <summary style={{ cursor: "pointer" }}>{L.cleanupPlanDetails}</summary>
          <div style={{ display: "grid", gap: 4, maxWidth: 420, paddingTop: 6 }}>
            <span>{L.cleanupPlanOther(plan.registry, plan.path)}</span>
            <span>{L.cleanupPlanBackup}</span>
            <span>{L.cleanupPlanEstimate}</span>
          </div>
        </details>
        {aiEnabled ? (
          <button
            style={css.btnSm}
            disabled={aiBusy || scan.items.length === 0}
            title={L.aiSettingsHint}
            onClick={onAiExplain}
          >
            {aiBusy ? L.aiExplaining : L.conclusionRefreshAi}
          </button>
        ) : (
          onOpenSettings && (
            <button
              style={css.btnSm}
              onClick={onOpenSettings}
              title={L.aiSettingsHint}
            >
              {L.conclusionEnableAi}
            </button>
          )
        )}
      </div>
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
