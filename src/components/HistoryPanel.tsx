import { t } from "../i18n";
import { CloseGlyph } from "./ui/Glyph";
import { VirtualList } from "./ui/VirtualList";
import { cssStyles as css } from "../styles";

export type HistoryRow = {
  id: string;
  app_name: string;
  deleted: number;
  failed: number;
  skipped?: number;
  backup_dir: string;
  created_at?: string;
};

function formatWhen(raw?: string): string {
  if (!raw) return "";
  const n = Number(raw);
  const d = Number.isFinite(n) && n > 1e8 ? new Date(n * 1000) : new Date(raw);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function dayKey(raw?: string): string {
  const when = formatWhen(raw);
  return when ? when.slice(0, 10) : "";
}

export function HistoryPanel({
  history,
  histQ,
  setHistQ,
  onDelete,
  onClearAll,
  onClose,
}: {
  history: HistoryRow[];
  histQ: string;
  setHistQ: (v: string) => void;
  onDelete: (id: string) => void;
  onClearAll: () => void;
  onClose: () => void;
}) {
  const L = t();
  const q = histQ.trim().toLowerCase();
  const rows = history.filter((h) => !q || h.app_name.toLowerCase().includes(q));

  const groups: { day: string; items: HistoryRow[] }[] = [];
  for (const h of rows) {
    const day = dayKey(h.created_at) || L.historyDayUnknown;
    const last = groups[groups.length - 1];
    if (last && last.day === day) last.items.push(h);
    else groups.push({ day, items: [h] });
  }
  // REV-UX-18: flatten day headers + entries into one windowed list.
  type HistoryUnit =
    | { kind: "header"; day: string }
    | { kind: "entry"; h: HistoryRow; idx: number };
  const units: HistoryUnit[] = [];
  for (const g of groups) {
    units.push({ kind: "header", day: g.day });
    g.items.forEach((h, idx) => units.push({ kind: "entry", h, idx }));
  }

  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <strong>{L.historyTitle}</strong>
        <span style={css.muted}>{L.historyTimelineHint}</span>
        <input
          style={{ ...css.input, maxWidth: 220, height: 32, marginLeft: "auto" }}
          placeholder={L.search}
          value={histQ}
          onChange={(e) => setHistQ(e.target.value)}
        />
        <button
          style={{ ...css.btnGhost, height: 30, color: "var(--danger)" }}
          disabled={history.length === 0}
          onClick={onClearAll}
        >
          {L.clearHistory}
        </button>
        <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose}>
          <CloseGlyph />
        </button>
      </div>
      <VirtualList
        items={units}
        height={280}
        estimateSize={72}
        rowGap={0}
        empty={<div style={css.muted}>{L.noHistory}</div>}
        keyOf={(u) =>
          u.kind === "header" ? `day-${u.day}` : u.h.id || `${u.h.app_name}-${u.h.created_at || u.idx}`
        }
        renderItem={(u) =>
          u.kind === "header" ? (
            <div
              style={{
                fontSize: 11,
                fontWeight: 700,
                color: "var(--muted)",
                letterSpacing: 0.4,
                padding: "6px 0 4px",
              }}
            >
              {u.day}
            </div>
          ) : (
            <div
              style={{
                display: "flex",
                gap: 10,
                alignItems: "center",
                flexWrap: "wrap",
                padding: "8px 10px",
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--surface-2)",
              }}
            >
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{ fontWeight: 600 }}>{u.h.app_name}</div>
                <div style={{ ...css.muted, fontFamily: "var(--mono)", fontSize: 11.5 }}>
                  {formatWhen(u.h.created_at) ? `${formatWhen(u.h.created_at)} · ` : ""}
                  {L.histRow(u.h.deleted, u.h.failed)}
                  {typeof u.h.skipped === "number" && u.h.skipped > 0
                    ? ` · ${L.reportSkipped} ${u.h.skipped}`
                    : ""}
                </div>
              </div>
              <button
                style={{ ...css.btnGhost, height: 26, padding: "0 8px", color: "var(--danger)" }}
                onClick={() => onDelete(u.h.id)}
              >
                {L.deleteHistory}
              </button>
            </div>
          )
        }
      />
    </div>
  );
}
