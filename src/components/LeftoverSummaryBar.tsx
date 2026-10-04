import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import type { LeftoverSummary } from "../lib/decision";

/** Risk/kind summary above leftover list — explain, do not scare. */
export function LeftoverSummaryBar({
  summary,
  scanning,
}: {
  summary: LeftoverSummary;
  scanning?: boolean;
}) {
  const L = t();
  if (summary.total === 0) return null;
  const buckets = [
    {
      id: "safe" as const,
      label: L.bucketSafe,
      count: summary.safe,
      color: "var(--ok-ink)",
      hint: L.bucketSafeHint,
    },
    {
      id: "suggest" as const,
      label: L.bucketSuggest,
      count: summary.suggest,
      color: "var(--warn-ink)",
      hint: L.bucketSuggestHint,
    },
    {
      id: "keep" as const,
      label: L.bucketKeep,
      count: summary.keep,
      color: "var(--danger-text)",
      hint: L.bucketKeepHint,
    },
  ];
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
      <span style={{ fontWeight: 650 }}>{L.leftoverSummaryTitle}</span>
      {buckets.map((b) => (
        <span
          key={b.id}
          title={b.hint}
          style={{
            ...css.chip,
            fontFamily: "inherit",
            color: b.count > 0 ? b.color : "var(--muted)",
            borderColor: b.count > 0 ? b.color : "var(--border)",
          }}
        >
          {b.label} {b.count}
        </span>
      ))}
      {/* Composition in one muted chip — informational, below the decision chips. */}
      {(() => {
        const order = ["dir", "file", "registry", "path"];
        const parts = order
          .map((k) => ({ k, n: summary.byKind.find((b) => b.kind === k)?.count ?? 0 }))
          .filter((x) => x.n > 0)
          .map((x) => `${L.kindLabel(x.k)} ${x.n}`);
        const extra = summary.byKind
          .filter((b) => !order.includes(b.kind))
          .map((b) => `${L.kindLabel(b.kind)} ${b.count}`);
        const all = [...parts, ...extra];
        if (all.length === 0) return null;
        return (
          <span
            title={L.kindSummaryHint}
            style={{ ...css.chip, fontFamily: "inherit", color: "var(--muted)" }}
          >
            {all.join(" · ")}
          </span>
        );
      })()}
      {scanning && <span style={css.muted}>{L.analyzing}</span>}
    </div>
  );
}
