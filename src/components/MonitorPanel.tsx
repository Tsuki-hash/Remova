import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type MonitorDiffData = {
  added_files: string[];
  added_reg_values: string[];
};

export function MonitorPanel({
  diff,
  onDismiss,
}: {
  diff: MonitorDiffData;
  onDismiss: () => void;
}) {
  const L = t();
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <strong>{L.monitorDiff}</strong>
      <div style={{ ...css.muted, margin: "6px 0" }}>
        files={diff.added_files.length} · reg={diff.added_reg_values.length}
      </div>
      <div style={{ maxHeight: 160, overflow: "auto" }}>
        {[...diff.added_files, ...diff.added_reg_values].slice(0, 80).map((line) => (
          <div key={line} style={{ wordBreak: "break-all", padding: "2px 0" }}>
            {line}
          </div>
        ))}
      </div>
      <button style={{ ...css.btnGhost, height: 30, marginTop: 8 }} onClick={onDismiss}>
        {L.batchDismiss}
      </button>
    </div>
  );
}
