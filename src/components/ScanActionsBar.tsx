import type { ScanResult } from "../types";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";

/** Scan preview toolbar: close / mode / dry-run / cleanup / AI explain. */
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
}) {
  const L = t();
  return (
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
      <span style={{ ...css.muted }}>
        {scan.items.length} · {scan.items.filter((i) => i.confidence === "confirmed").length}{" "}
        {L.confirmed}
      </span>
      {scanning && <span style={{ ...css.muted }}>{L.analyzing}</span>}
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
      <button style={css.btnGhost} disabled={dryRunning || selectedPaths.size === 0} onClick={onDryRun}>
        {L.dryRun}
      </button>
      {/* REV-UX-03: scope summary before the destructive CTA */}
      {selectedPaths.size > 0 && (
        <span style={{ ...css.muted, fontSize: 12 }} title={L.dangerScopeHint}>
          {L.dangerScope(selectedPaths.size)}
        </span>
      )}
      <button
        style={{ ...css.btn, background: "var(--danger)", color: "#1a0505" }}
        disabled={dryRunning || selectedPaths.size === 0 || busy}
        onClick={onCleanup}
        title={L.dangerScopeHint}
      >
        {L.cleanup} ({selectedPaths.size})
      </button>
      {/* AI explain runs automatically after scan (decision layer); keep manual re-run if enabled */}
      {aiEnabled && (
        <button
          style={css.btnSm}
          disabled={aiBusy || scan.items.length === 0}
          title={L.aiSettingsHint}
          onClick={onAiExplain}
        >
          {aiBusy ? L.aiExplaining : L.conclusionRefreshAi}
        </button>
      )}
    </div>
  );
}
