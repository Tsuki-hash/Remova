import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type MonitorDiffData = {
  added_files: string[];
  added_reg_values: string[];
  /** REV-BE-10: entries beyond the diff caps — shown as an honest truncation note. */
  files_truncated?: number;
  reg_truncated?: number;
};

export function MonitorPanel({
  diff,
  monitoring,
  onToCleanup,
  onDismiss,
  onClose,
}: {
  diff: MonitorDiffData;
  monitoring?: boolean;
  onToCleanup: () => void;
  onDismiss: () => void;
  onClose?: () => void;
}) {
  const L = t();
  const total = diff.added_files.length + diff.added_reg_values.length;
  const truncated = (diff.files_truncated ?? 0) + (diff.reg_truncated ?? 0);
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <strong>{monitoring ? L.monitorStop : L.monitorInstall}</strong>
        <span style={css.muted}>
          {total === 0
            ? monitoring
              ? L.monitorInstallHint
              : L.monitorEmpty
            : L.monitorDiffCounts(diff.added_files.length, diff.added_reg_values.length)}
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
          {onClose && (
            <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose}>
              {L.panelClose}
            </button>
          )}
        </div>
      </div>
      {truncated > 0 && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>
          {L.monitorTruncated(diff.files_truncated ?? 0, diff.reg_truncated ?? 0)}
        </div>
      )}
      <div style={{ maxHeight: 160, overflow: "auto", marginTop: 8 }}>
        {total === 0 ? (
          <div style={{ ...css.muted, padding: "8px 0" }}>{L.monitorEmptyHint}</div>
        ) : (
          [...diff.added_files, ...diff.added_reg_values].map((line, i) => (
            <div
              key={`${i}-${line}`}
              style={{ wordBreak: "break-all", padding: "2px 0" }}
            >
              {line}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
