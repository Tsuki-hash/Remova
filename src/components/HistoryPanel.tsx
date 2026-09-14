import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type HistoryRow = {
  app_name: string;
  deleted: number;
  failed: number;
  backup_dir: string;
};

export function HistoryPanel({
  history,
  histQ,
  setHistQ,
  onClose,
}: {
  history: HistoryRow[];
  histQ: string;
  setHistQ: (v: string) => void;
  onClose: () => void;
}) {
  const L = t();
  const q = histQ.trim().toLowerCase();
  const rows = history.filter((h) => !q || h.app_name.toLowerCase().includes(q));
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <strong>{L.historyTitle}</strong>
      <input
        style={{ ...css.input, maxWidth: 220, height: 32, marginLeft: 12 }}
        placeholder={L.search}
        value={histQ}
        onChange={(e) => setHistQ(e.target.value)}
      />
      <button style={{ marginLeft: 12, ...css.btnGhost }} onClick={onClose}>
        ×
      </button>
      <div style={{ maxHeight: 160, overflow: "auto", marginTop: 8 }}>
        {rows.length === 0 && <div>{L.noHistory}</div>}
        {rows.map((h, i) => (
          <div key={i}>
            {h.app_name} · {L.histRow(h.deleted, h.failed)}
            {h.backup_dir ? ` · ${h.backup_dir}` : ""}
          </div>
        ))}
      </div>
    </div>
  );
}
