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
      color: "var(--ok)",
      hint: L.bucketSafeHint,
    },
    {
      id: "suggest" as const,
      label: L.bucketSuggest,
      count: summary.suggest,
      color: "var(--warn)",
      hint: L.bucketSuggestHint,
    },
    {
      id: "keep" as const,
      label: L.bucketKeep,
      count: summary.keep,
      color: "var(--danger)",
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
      {summary.byKind.slice(0, 5).map((k) => (
        <span key={k.kind} style={{ ...css.chip, fontFamily: "inherit" }}>
          {k.kind} {k.count}
        </span>
      ))}
      {scanning && <span style={css.muted}>{L.analyzing}</span>}
    </div>
  );
}
