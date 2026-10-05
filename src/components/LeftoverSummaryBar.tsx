import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import type { LeftoverSummary } from "../lib/decision";

/** Filter chips above the leftover list: select-safe action + risk filters + composition. */
export function LeftoverSummaryBar({
  summary,
  scanning,
  riskFilter,
  onSelectSafe,
  onShowConfirm,
  onShowKeep,
}: {
  summary: LeftoverSummary;
  scanning?: boolean;
  riskFilter?: "confirm" | "keep" | null;
  onSelectSafe?: () => void;
  onShowConfirm?: () => void;
  onShowKeep?: () => void;
}) {
  const L = t();
  if (summary.total === 0 && !scanning) return null;
  const buckets = [
    {
      id: "safe" as const,
      label: L.conclusionCleanSafe(summary.safe),
      count: summary.safe,
      hint: L.bucketSafeHint,
      active: false,
      onClick: onSelectSafe,
    },
    {
      id: "confirm" as const,
      label: L.conclusionShowConfirm(summary.suggest),
      count: summary.suggest,
      hint: L.bucketSuggestHint,
      active: riskFilter === "confirm",
      onClick: onShowConfirm,
    },
    {
      id: "keep" as const,
      label: L.conclusionShowKeep(summary.keep),
      count: summary.keep,
      hint: L.bucketKeepHint,
      active: riskFilter === "keep",
      onClick: onShowKeep,
    },
  ];
  const order = ["dir", "file", "registry", "path"];
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: 8,
        alignItems: "center",
        padding: "8px 14px",
        borderBottom: "1px solid var(--border)",
        background: "var(--th-bg)",
        flexShrink: 0,
        fontSize: 12,
      }}
    >
      {buckets.map((b) => b.onClick ? (
        <button
          key={b.id}
          type="button"
          title={b.hint}
          aria-pressed={b.id === "safe" ? undefined : b.active}
          disabled={b.count === 0}
          onClick={b.onClick}
          style={{
            ...css.chip,
            fontFamily: "inherit",
            cursor: b.count === 0 ? "not-allowed" : "pointer",
            color: b.active
              ? "var(--accent-text)"
              : b.count > 0
                ? b.id === "safe"
                  ? "var(--ok-ink)"
                  : b.id === "confirm"
                    ? "var(--warn-ink)"
                    : "var(--muted)"
                : "var(--muted)",
            borderColor: b.active
              ? "var(--accent)"
              : b.count > 0
                ? b.id === "safe"
                  ? "var(--ok-ink)"
                  : b.id === "confirm"
                    ? "var(--warn)"
                    : "var(--border)"
                : "var(--border)",
            background: b.active ? "var(--accent-soft)" : "transparent",
            opacity: b.count === 0 ? 0.55 : 1,
          }}
        >
          {b.label}
        </button>
      ) : (
        <span key={b.id} title={b.hint} style={{ ...css.chip, color: "var(--muted)" }}>
          {b.id === "safe" ? L.bucketSafe : b.id === "confirm" ? L.bucketSuggest : L.bucketKeep}
          {` (${b.count})`}
        </span>
      ))}
      <span style={{ flex: 1 }} />
      {(() => {
        const parts2 = summary.byKind
          .filter((b) => order.includes(b.kind))
          .sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind))
          .map((b) => `${L.kindLabel(b.kind)} ${b.count}`);
        if (parts2.length === 0) return null;
        return (
          <span
            title={L.kindSummaryHint}
            style={{ ...css.chip, fontFamily: "inherit", color: "var(--muted)" }}
          >
            {parts2.join(" · ")}
          </span>
        );
      })()}
      {scanning && <span style={css.muted}>{L.analyzing}</span>}
    </div>
  );
}
