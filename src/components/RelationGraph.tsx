import { t } from "../i18n";
import { summarizeLeftovers } from "../lib/decision";
import type { ScanResult } from "../types";

/** Simple SVG relation graph (app → kind buckets). */
export function RelationGraph({ scan }: { scan: ScanResult }) {
  const L = t();
  const s = summarizeLeftovers(scan.items);
  const kinds = s.byKind.slice(0, 4);
  const w = 340;
  const rowH = 28;
  const h = 48 + kinds.length * rowH + 12;
  const rootX = 16;
  const rootY = 28;
  const nodeX = 160;
  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      width="100%"
      height={h}
      role="img"
      aria-label={L.relationTitle}
      style={{ display: "block" }}
    >
      <rect x={rootX} y={rootY - 14} width={130} height={28} rx={8} fill="var(--accent-soft)" stroke="var(--accent)" strokeWidth={0.5} />
      <text x={rootX + 65} y={rootY + 4} textAnchor="middle" dominantBaseline="central" fontSize={11} fill="var(--fg)">
        {scan.app_name.length > 16 ? `${scan.app_name.slice(0, 14)}…` : scan.app_name}
      </text>
      {kinds.map((k, i) => {
        const y = 48 + i * rowH + 8;
        return (
          <g key={k.kind}>
            <path
              d={`M ${rootX + 130} ${rootY} C ${rootX + 150} ${rootY}, ${nodeX - 20} ${y}, ${nodeX} ${y}`}
              fill="none"
              stroke="var(--border-strong)"
              strokeWidth={1}
            />
            <rect x={nodeX} y={y - 12} width={150} height={24} rx={6} fill="var(--surface-2)" stroke="var(--border)" strokeWidth={0.5} />
            <text x={nodeX + 10} y={y + 1} dominantBaseline="central" fontSize={11} fill="var(--fg)">
              {k.kind}
            </text>
            <text x={nodeX + 140} y={y + 1} textAnchor="end" dominantBaseline="central" fontSize={11} fill="var(--muted)">
              {k.count}
            </text>
          </g>
        );
      })}
      <text x={16} y={h - 6} fontSize={10} fill="var(--muted)">
        {L.bucketSafe} {s.safe} · {L.bucketSuggest} {s.suggest} · {L.bucketKeep} {s.keep}
      </text>
    </svg>
  );
}
