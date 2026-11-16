import { t } from "../i18n";
import { cssStyles as css } from "../styles";
// single source for the monitor DTO — no component-local duplicate.
import type { MonitorDiff } from "../lib/api";

export function MonitorPanel({
  diff,
  monitoring,
  stopping,
  onToggle,
  onToCleanup,
  onDismiss,
  onClose,
}: {
  diff: MonitorDiff | null;
  monitoring?: boolean;
  stopping?: boolean;
  onToggle?: () => void;
  onToCleanup: () => void;
  onDismiss: () => void;
  onClose?: () => void;
}) {
  const L = t();
  const result = monitoring || stopping ? null : diff;
  const files = result?.added_files.length ?? 0;
  const registryValues = result?.added_reg_values.length ?? 0;
  const total = files + registryValues;
  const truncated = (result?.files_truncated ?? 0) + (result?.reg_truncated ?? 0);
  return (
    <div style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <header className="remova-tool-heading">
        <h2>{L.monitorInstall}</h2>
        <div className="remova-tool-actions">
          {!result && onToggle && (
            <button type="button" style={css.btn} disabled={stopping} onClick={onToggle}>
              {monitoring ? L.monitorStop : L.monitorStartAction}
            </button>
          )}
          {total > 0 && (
            <button type="button" style={css.btn} disabled={result?.walk_degraded} onClick={onToCleanup}>
              {L.monitorToCleanup}
            </button>
          )}
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
      {!stopping && <p className="remova-tool-note" role="status">
        {monitoring ? L.monitorActive
          : !result ? L.monitorStartHint
            : total === 0 ? L.monitorEmpty : L.monitorDiffCounts(files, registryValues)}
      </p>}
      {result && truncated > 0 && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>
          {L.monitorTruncated(result.files_truncated ?? 0, result.reg_truncated ?? 0)}
        </div>
      )}
      {result?.walk_degraded && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>
          {L.monitorDegraded}
        </div>
      )}
      {result && total > 0 && (
        <div
          tabIndex={0}
          role="group"
          aria-label={L.monitorDiff}
          style={{ maxHeight: 160, overflow: "auto", marginTop: 8 }}
        >
          {[...result.added_files, ...result.added_reg_values].map((line, i) => (
            <div
              key={`${i}-${line}`}
              style={{ wordBreak: "break-all", padding: "2px 0" }}
            >
              {line}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
