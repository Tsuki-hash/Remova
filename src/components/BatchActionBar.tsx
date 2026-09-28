import { t } from "../i18n";
import { cssStyles as css } from "../styles";

/** Bottom bar when multi-select has rows: cancel/clear or start batch uninstall. */
export function BatchActionBar({
  count,
  batching,
  onCancelOrClear,
  onStart,
}: {
  count: number;
  batching: boolean;
  onCancelOrClear: () => void;
  onStart: () => void;
}) {
  const L = t();
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "12px 14px",
        borderTop: "1px solid var(--border)",
        background: "var(--surface-2)",
        flexShrink: 0,
      }}
    >
      <span style={{ fontSize: 13, fontWeight: 600 }}>
        {L.batchUninstall} · {count}
      </span>
      {/* REV-UX-03: destructive batch CTA shows scope / backup / irreversibility */}
      <span style={{ color: "var(--muted)", fontSize: 12 }} title={L.dangerScopeHint}>
        {L.dangerScope(count)}
      </span>
      <button style={css.btnGhost} onClick={onCancelOrClear}>
        {batching ? L.batchCancel : L.batchDismiss}
      </button>
      <button
        style={{ ...css.btn, marginLeft: "auto", background: "var(--danger)", color: "#fff" }}
        disabled={batching}
        title={L.dangerScopeHint}
        onClick={onStart}
      >
        {`${L.batchUninstall} (${count})`}
      </button>
    </div>
  );
}
