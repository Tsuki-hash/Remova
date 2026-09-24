import { t } from "../i18n";
import { CloseGlyph } from "./ui/Glyph";
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
          style={{ ...css.btnGhost, height: 30, color: "#b91c1c" }}
          disabled={history.length === 0}
          onClick={onClearAll}
        >
          {L.clearHistory}
        </button>
        <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose}>
          <CloseGlyph />
        </button>
      </div>
      <div style={{ maxHeight: 280, overflow: "auto", marginTop: 10 }}>
        {rows.length === 0 && <div style={css.muted}>{L.noHistory}</div>}
        {groups.map((g) => (
          <div key={g.day} style={{ marginBottom: 10 }}>
            <div
              style={{
                fontSize: 11,
                fontWeight: 700,
                color: "var(--muted)",
                letterSpacing: 0.4,
                marginBottom: 4,
              }}
            >
              {g.day}
            </div>
            {g.items.map((h, i) => (
              <div
                key={h.id || `${h.app_name}-${h.created_at || i}`}
                style={{
                  display: "flex",
                  gap: 10,
                  alignItems: "center",
                  flexWrap: "wrap",
                  padding: "8px 10px",
                  borderRadius: 8,
                  border: "1px solid var(--border)",
                  background: "var(--surface-2)",
                  marginBottom: 6,
                }}
              >
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div style={{ fontWeight: 600 }}>{h.app_name}</div>
                  <div style={{ ...css.muted, fontFamily: "var(--mono)", fontSize: 11.5 }}>
                    {formatWhen(h.created_at) ? `${formatWhen(h.created_at)} · ` : ""}
                    {L.histRow(h.deleted, h.failed)}
                    {typeof h.skipped === "number" && h.skipped > 0
                      ? ` · ${L.reportSkipped} ${h.skipped}`
                      : ""}
                  </div>
                </div>
                <button
                  style={{ ...css.btnGhost, height: 26, padding: "0 8px", color: "#b91c1c" }}
                  onClick={() => onDelete(h.id)}
                >
                  {L.deleteHistory}
                </button>
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
