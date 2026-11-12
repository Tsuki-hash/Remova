import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type BatchStatus = "ok" | "failed" | "skipped" | "reboot" | "not_run";
type SkipReason = "cancelled" | "protected" | "no-leftovers";
export type BatchItemResult = {
  key: string;
  name: string;
  detail: string;
  historyWarning?: string;
  /** Preserved across residual retries, including later analyze failures. */
  officialDone?: boolean;
} & ({ status: "skipped"; skipReason: SkipReason } | { status: Exclude<BatchStatus, "skipped"> });

export function BatchProgress({
  index,
  total,
  current,
}: {
  index: number;
  total: number;
  current: string;
}) {
  const L = t();
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: "10px 14px", fontSize: 13 }}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
        <span>
          {index}/{total} · {current}
        </span>
        <span style={css.muted}>{L.batchCancelHint}</span>
      </div>
      <div
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={Math.max(total, 1)}
        aria-valuenow={index}
        aria-valuetext={`${index}/${total}`}
        style={{
          marginTop: 8,
          height: 6,
          borderRadius: 3,
          background: "var(--th-bg)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            width: `${total ? (index / total) * 100 : 0}%`,
            height: "100%",
            background: "var(--accent)",
            transition: "width 0.2s",
          }}
        />
      </div>
    </div>
  );
}

export function BatchSummaryPanel({
  results,
  onRetryFailed,
  onDismiss,
}: {
  results: BatchItemResult[];
  onRetryFailed: () => void;
  onDismiss: () => void;
}) {
  const L = t();
  const skipLabels = {
    cancelled: L.batchSkippedCancelled,
    protected: L.batchSkippedBlocked,
    "no-leftovers": L.batchSkippedNone,
  };
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 8 }}>
        <strong>{L.batchSummary}</strong>
        <span style={css.muted}>
          {L.batchOk} {results.filter((r) => r.status === "ok").length} · {L.batchFailed}{" "}
          {results.filter((r) => r.status === "failed").length} · {L.batchReboot}{" "}
          {results.filter((r) => r.status === "reboot").length} · {L.batchSkipped}{" "}
          {results.filter((r) => r.status === "skipped").length} · {L.batchNotRun}{" "}
          {results.filter((r) => r.status === "not_run").length}
        </span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          {results.some((r) => r.status === "failed") && (
            <button style={{ ...css.btnGhost, height: 32 }} onClick={onRetryFailed}>
              {L.batchRetryFailed}
            </button>
          )}
          <button style={{ ...css.btnGhost, height: 32 }} onClick={onDismiss}>
            {L.batchDismiss}
          </button>
        </div>
      </div>
      <div style={{ maxHeight: 180, overflow: "auto" }}>
        {results.map((r) => (
          <div key={r.key} style={{ padding: "4px 0", borderBottom: "1px solid var(--border)" }}>
            <span
              style={{
                color:
                  r.status === "failed"
                    ? "var(--danger)"
                    : r.status === "reboot"
                      ? "var(--warn-ink)"
                      : r.status === "ok"
                        ? "var(--accent)"
                        : "var(--muted)",
                marginRight: 8,
              }}
            >
              {r.status === "skipped"
                ? skipLabels[r.skipReason]
                : r.status === "ok"
                  ? L.batchOk
                  : r.status === "failed"
                    ? L.batchFailed
                    : r.status === "reboot"
                      ? L.batchReboot
                      : L.batchNotRun}
            </span>
            {r.name}
            {r.detail ? <span style={css.muted}> · {r.detail}</span> : null}
            {r.historyWarning ? <span style={css.muted}> · {r.historyWarning}</span> : null}
          </div>
        ))}
      </div>
    </div>
  );
}
