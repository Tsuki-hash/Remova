import type { ScanProgressState } from "../hooks/useAssociationScan";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";

/** Honest elapsed time: sub-10s keeps one decimal, minutes keep mm:ss shape. */
export function formatElapsed(ms: number): string {
  if (ms < 10_000) return `${(ms / 1000).toFixed(1)}s`;
  const total = Math.round(ms / 1000);
  if (total < 60) return `${total}s`;
  return `${Math.floor(total / 60)}m${String(total % 60).padStart(2, "0")}s`;
}

export function ScanProgressBar({ progress, onCancel, title, cancelledLabel, failedLabel }: {
  progress: ScanProgressState | null; onCancel: () => Promise<void>;
  title?: string; cancelledLabel?: string; failedLabel?: string;
}) {
  if (!progress || progress.status === "complete") return null;
  const L = t();
  const running = progress.status === "running" || progress.status === "stopping";
  const elapsed = L.scanElapsed(formatElapsed(progress.elapsedMs));
  return <div style={{ ...css.card, padding: 10, marginBottom: 10 }}>
    <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
      {title && <strong>{title}</strong>}
      {/* the stage changes every poll (250 ms); announcing it drowned
          out everything else. Only terminal states stay a live status. */}
      <span role={running ? undefined : "status"}>{progress.status === "cancelled" ? cancelledLabel ?? L.scanCancelledIncomplete
        : progress.status === "failed" ? failedLabel ?? L.scanFailedIncomplete
        : progress.status === "stopping" ? L.scanStopping : L.scanStage(progress.stage)}</span>
      {running && <span style={css.muted}>{L.scanFound(progress.found)}</span>}
      {/* elapsed stays visible in terminal states so a cancelled or degraded
          scan still reports how long it actually ran. */}
      <span style={css.muted}>{elapsed}</span>
      {running && <button style={{ ...css.btnGhost, marginLeft: "auto" }} disabled={progress.status === "stopping"}
        onClick={() => void onCancel().catch(e => toast.error(formatError(e, "analyze")))}>{L.cancelScan}</button>}
    </div>
    {running && <progress aria-label={L.scanInProgress} style={{ width: "100%", height: 4, marginTop: 8 }} />}
  </div>;
}
