import type { ScanProgressState } from "../hooks/useAssociationScan";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";

export function ScanProgressBar({ progress, onCancel, title, cancelledLabel, failedLabel }: {
  progress: ScanProgressState | null; onCancel: () => Promise<void>;
  title?: string; cancelledLabel?: string; failedLabel?: string;
}) {
  if (!progress || progress.status === "complete") return null;
  const L = t();
  const running = progress.status === "running" || progress.status === "stopping";
  return <div style={{ ...css.card, padding: 10, marginBottom: 10 }}>
    <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
      {title && <strong>{title}</strong>}
      <span role="status">{progress.status === "cancelled" ? cancelledLabel ?? L.scanCancelledIncomplete
        : progress.status === "failed" ? failedLabel ?? L.scanFailedIncomplete
        : progress.status === "stopping" ? L.scanStopping : L.scanStage(progress.stage)}</span>
      {running && <span style={css.muted}>{L.scanFound(progress.found)}</span>}
      {running && <button style={{ ...css.btnGhost, marginLeft: "auto" }} disabled={progress.status === "stopping"}
        onClick={() => void onCancel().catch(e => toast.error(formatError(e, "analyze")))}>{L.cancelScan}</button>}
    </div>
    {running && <progress aria-label={L.scanInProgress} style={{ width: "100%", height: 4, marginTop: 8 }} />}
  </div>;
}
