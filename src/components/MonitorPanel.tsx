import { t } from "../i18n";
import { cssStyles as css } from "../styles";
// single source for the monitor DTO — no component-local duplicate.
import type { MonitorDiff } from "../lib/api";

export function MonitorPanel({
  diff,
  monitoring,
  stopping,
  onStop,
  onToCleanup,
  onDismiss,
  onClose,
}: {
  diff: MonitorDiff;
  monitoring?: boolean;
  stopping?: boolean;
  onStop?: () => void;
  onToCleanup: () => void;
  onDismiss: () => void;
  onClose?: () => void;
}) {
  const L = t();
  const total = diff.added_files.length + diff.added_reg_values.length;
  const truncated = (diff.files_truncated ?? 0) + (diff.reg_truncated ?? 0);
  return (
    <div style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <header className="remova-tool-heading">
        <h2>{L.monitorInstall}</h2>
        <div className="remova-tool-actions">
          {monitoring && onStop && (
            <button type="button" style={css.btn} disabled={stopping} onClick={onStop}>
              {L.monitorStop}
            </button>
          )}
          <button
            style={{ ...css.btn, height: 30, opacity: total === 0 || diff.walk_degraded ? 0.5 : 1 }}
            disabled={total === 0 || diff.walk_degraded}
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
      </header>
      <p className="remova-tool-note" role="status">
        {monitoring ? L.monitorActive
          : total === 0 ? L.monitorEmpty
            : L.monitorDiffCounts(diff.added_files.length, diff.added_reg_values.length)}
      </p>
      {truncated > 0 && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>
          {L.monitorTruncated(diff.files_truncated ?? 0, diff.reg_truncated ?? 0)}
        </div>
      )}
      {diff.walk_degraded && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>
          {L.monitorDegraded}
        </div>
      )}
      <div
        tabIndex={0}
        role="group"
        aria-label={L.monitorDiff}
        style={{ maxHeight: 160, overflow: "auto", marginTop: 8 }}
      >
        {total === 0 ? (
          !monitoring && <div className="remova-tool-note">{L.monitorEmptyHint}</div>
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
