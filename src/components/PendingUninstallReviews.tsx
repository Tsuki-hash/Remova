import { useEffect, useRef, useState } from "react";
import { t } from "../i18n";
import { api, type PendingUninstallReview, type PendingUninstallReviewStatus } from "../lib/api";
import { cssStyles as css } from "../styles";

export function PendingUninstallReviews({ busy, onGoOrphans }: {
  busy: boolean;
  onGoOrphans: () => void;
}) {
  const L = t();
  const [rows, setRows] = useState<PendingUninstallReview[]>([]);
  const [statuses, setStatuses] = useState<Record<number, PendingUninstallReviewStatus>>({});
  const [active, setActive] = useState<number | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [retry, setRetry] = useState(0);
  const operation = useRef(false);
  useEffect(() => {
    if (busy || active !== null) return;
    let live = true;
    void (async () => {
      try {
        const data = await api.pendingUninstallReviews();
        if (!live) return;
        setRows(data ?? []);
        setLoadFailed(false);
      } catch {
        if (live) setLoadFailed(true);
      }
    })();
    return () => { live = false; };
  }, [busy, active, retry]);

  async function review(id: number, dismiss: boolean) {
    if (busy || operation.current) return;
    operation.current = true;
    setActive(id);
    try {
      if (dismiss) {
        await api.dismissPendingUninstallReview(id);
        setRows(current => current.filter(row => row.id !== id));
        setStatuses(current => {
          const rest = { ...current };
          delete rest[id];
          return rest;
        });
      } else {
        const status = await api.checkPendingUninstallReview(id);
        setStatuses(current => ({ ...current, [id]: status }));
      }
    } catch {
      setStatuses(current => ({ ...current, [id]: "unverified" }));
    } finally {
      operation.current = false;
      setActive(null);
    }
  }

  if (!rows.length && !loadFailed) return null;
  const labels: Record<PendingUninstallReviewStatus, string> = {
    awaiting_restart: L.pendingReviewAwaitRestart,
    installed: L.pendingReviewInstalled,
    removed: L.pendingReviewRemoved,
    unverified: L.pendingReviewUnverified,
  };
  return (
    <section aria-label={L.pendingReviewTitle} style={{ ...css.card, marginBottom: 10, padding: 12 }}>
      <strong>{L.pendingReviewTitle}</strong>
      {loadFailed && <p role="status">
        {L.pendingReviewLoadFailed}{" "}
        <button type="button" style={css.btnGhost} disabled={busy} onClick={() => setRetry(value => value + 1)}>{L.retry}</button>
      </p>}
      {rows.map(row => {
        const status = statuses[row.id];
        return <div key={row.id} style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center", marginTop: 8 }}>
          <div role="status" style={{ flex: "1 1 240px", overflowWrap: "anywhere" }}>
            <strong>{row.name}{row.version && ` · ${row.version}`}</strong>
            <div style={css.muted}>{active === row.id ? L.loadingGeneric : status ? labels[status] : L.pendingReviewHint}</div>
          </div>
          <button type="button" style={css.btnGhost} disabled={busy || active !== null}
            onClick={() => void review(row.id, false)}>{L.pendingReviewCheck}</button>
          {status === "removed" && <button type="button" style={css.btnGhost} onClick={onGoOrphans}>{L.navOrphans}</button>}
          <button type="button" style={css.btnGhost} disabled={busy || active !== null}
            onClick={() => void review(row.id, true)}>{L.pendingReviewDismiss}</button>
        </div>;
      })}
    </section>
  );
}
