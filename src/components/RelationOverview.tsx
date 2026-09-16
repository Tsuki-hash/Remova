import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { originLabel, summarizeLeftovers } from "../lib/decision";
import type { ScanResult } from "../types";

/** Text relation summary in the app drawer when a scan is loaded (graph-lite). */
export function RelationOverview({ scan }: { scan: ScanResult }) {
  const L = t();
  const s = summarizeLeftovers(scan.items);
  const leaves = scan.items.slice(0, 8);
  return (
    <div style={{ ...css.card, padding: "10px 12px", fontSize: 12.5 }}>
      <div style={{ fontWeight: 650, marginBottom: 6 }}>{L.relationTitle}</div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 8 }}>
        {s.byKind.map((k) => (
          <span key={k.kind} style={{ ...css.chip, fontFamily: "inherit" }}>
            {k.kind} {k.count}
          </span>
        ))}
        <span style={{ ...css.chip, fontFamily: "inherit", color: "var(--ok)" }}>
          {L.bucketSafe} {s.safe}
        </span>
        <span style={{ ...css.chip, fontFamily: "inherit", color: "var(--warn)" }}>
          {L.bucketSuggest} {s.suggest}
        </span>
        <span style={{ ...css.chip, fontFamily: "inherit", color: "var(--danger)" }}>
          {L.bucketKeep} {s.keep}
        </span>
      </div>
      <div style={{ fontFamily: "var(--mono)", fontSize: 11.5, color: "var(--muted)", lineHeight: 1.55 }}>
        <div>{scan.app_name}</div>
        {leaves.map((it, i) => (
          <div key={it.path} style={{ paddingLeft: 12 }}>
            {i === leaves.length - 1 ? "└─ " : "├─ "}
            {originLabel(it.path)}
            <span style={{ opacity: 0.7 }}> · {it.kind}</span>
          </div>
        ))}
        {scan.items.length > leaves.length && (
          <div style={{ paddingLeft: 12 }}>… +{scan.items.length - leaves.length}</div>
        )}
      </div>
    </div>
  );
}
