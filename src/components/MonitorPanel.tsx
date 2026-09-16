import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type MonitorDiffData = {
  added_files: string[];
  added_reg_values: string[];
};

export function MonitorPanel({
  diff,
  onToCleanup,
  onDismiss,
}: {
  diff: MonitorDiffData;
  onToCleanup: () => void;
  onDismiss: () => void;
}) {
  const L = t();
  const total = diff.added_files.length + diff.added_reg_values.length;
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <strong>{L.monitorDiff}</strong>
        <span style={css.muted}>
          files={diff.added_files.length} · reg={diff.added_reg_values.length}
        </span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button
            style={{ ...css.btn, height: 30, opacity: total === 0 ? 0.5 : 1 }}
            disabled={total === 0}
            onClick={onToCleanup}
          >
            {L.monitorToCleanup}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onDismiss}>
            {L.batchDismiss}
          </button>
        </div>
      </div>
      <div style={{ maxHeight: 160, overflow: "auto", marginTop: 8 }}>
        {[...diff.added_files, ...diff.added_reg_values].slice(0, 80).map((line) => (
          <div key={line} style={{ wordBreak: "break-all", padding: "2px 0" }}>
            {line}
          </div>
        ))}
      </div>
    </div>
  );
}
