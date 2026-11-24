import { BackButton } from "./ui/BackButton";
import { InstallTrialHistory } from "./InstallTrialHistory";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
// single source for the monitor DTO — no component-local duplicate.
import type { MonitorDiff } from "../lib/api";

export function MonitorPanel({
  diff,
  monitoring,
  recovery,
  onDiscardRecovery,
  stopping,
  onToggle,
  onToCleanup,
  onDismiss,
  onClose,
}: {
  diff: MonitorDiff | null;
  monitoring?: boolean;
  recovery?: "recovered" | "unavailable" | null;
  onDiscardRecovery?: () => void;
  stopping?: boolean;
  onToggle?: (trialName?: string) => void;
  onToCleanup: () => void;
  onDismiss: () => void;
  onClose?: () => void;
}) {
  const L = t();
  const result = monitoring || stopping ? null : diff;
  const newSession = !result && !monitoring && !recovery;
  const files = result?.added_files.length ?? 0;
  const registryValues = result?.added_reg_values.length ?? 0;
  const total = files + registryValues;
  const truncated = (result?.files_truncated ?? 0) + (result?.reg_truncated ?? 0);
  return (
    <div style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <header className="remova-tool-heading">
        <h2>{L.monitorInstall}</h2>
        <div className="remova-tool-actions">
          {!newSession && !result && recovery !== "unavailable" && onToggle && (
            <button type="button" style={css.btn} disabled={stopping}
              onClick={() => onToggle()}>
              {monitoring ? L.monitorStop : recovery === "recovered" ? L.monitorReviewRecovery : L.monitorStartAction}
            </button>
          )}
          {recovery && onDiscardRecovery && (
            <button type="button" style={css.btnGhost} disabled={stopping} onClick={onDiscardRecovery}>{L.monitorDiscardRecovery}</button>
          )}
          {total > 0 && !result?.recovered_read_only && !result?.trial_read_only && (
            <button type="button" style={css.btn} disabled={result?.walk_degraded} onClick={onToCleanup}>
              {L.monitorToCleanup}
            </button>
          )}
          {(result || !onClose) && (
            <button type="button" style={css.btnGhost} onClick={onDismiss}>
              {onClose ? L.monitorClearResult : L.batchDismiss}
            </button>
          )}
          {onClose && <BackButton onClick={onClose} />}
        </div>
      </header>
      {newSession && onToggle && (
        <form style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "end", marginTop: 12 }}
          onSubmit={event => {
            event.preventDefault();
            const name = new FormData(event.currentTarget).get("trialName");
            if (!stopping && typeof name === "string") onToggle(name.trim() || undefined);
          }}>
          <label style={{ display: "grid", gap: 6, flex: "1 1 200px", maxWidth: 360 }}>
            {L.trialName}
            <input name="trialName" style={{ ...css.input, minWidth: 0 }} maxLength={120} readOnly={stopping} />
          </label>
          <button type="submit" style={css.btn} disabled={stopping}>{L.monitorStartAction}</button>
        </form>
      )}
      {!stopping && <p className="remova-tool-note" role="status">
        {monitoring ? L.monitorActive
          : recovery === "recovered" ? L.monitorRecoveryHint
          : recovery === "unavailable" ? L.monitorRecoveryUnavailable
          : !result ? L.monitorStartHint
            : total === 0 ? L.monitorEmpty : L.monitorDiffCounts(files, registryValues)}
      </p>}
      {result?.recovered_read_only && <p className="remova-tool-note">{L.monitorRecoveryReadOnly}</p>}
      {result?.trial_read_only && <p className="remova-tool-note">{L.trialReadOnly}</p>}
      {result?.trial_history_save_failed && <p role="status" className="remova-tool-note">{L.trialHistoryNotSaved}</p>}
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
      {onClose && <InstallTrialHistory refresh={diff} />}
    </div>
  );
}
