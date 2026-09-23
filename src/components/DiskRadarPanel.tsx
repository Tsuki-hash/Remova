import { useEffect, useState } from "react";
import { api, type DiskUsage } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import type { DirSizeRow } from "../types";

/** Disk usage radar — read-only. Open path / drill down; never deletes. */
export function DiskRadarPanel({
  onClose,
  onError,
}: {
  onClose: () => void;
  onError: (msg: string) => void;
}) {
  const L = t();
  const [rows, setRows] = useState<DirSizeRow[] | null>(null);
  const [usage, setUsage] = useState<DiskUsage | null>(null);
  const [busy, setBusy] = useState(false);
  const [crumbs, setCrumbs] = useState<DirSizeRow[]>([]);

  const load = async (parent?: string) => {
    setBusy(true);
    try {
      const list = parent ? await api.listDirChildren(parent) : await api.listTopDirSizes();
      setRows(list);
    } catch (e) {
      onError(formatError(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void (async () => {
      try {
        setUsage(await api.diskUsage());
      } catch {
        /* optional */
      }
      await load();
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <strong>{L.diskRadarTitle}</strong>
        <span style={css.muted}>{L.diskRadarHint}</span>
        {usage && (
          <span style={{ ...css.muted, fontFamily: "var(--mono)", fontSize: 11.5 }}>
            {usage.drive || ""} {formatSize(Math.round(usage.free_gb * 1024 * 1024))} /{" "}
            {formatSize(Math.round(usage.total_gb * 1024 * 1024))}
          </span>
        )}
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          {crumbs.length > 0 && (
            <button
              style={{ ...css.btnGhost, height: 30 }}
              onClick={() => {
                const next = crumbs.slice(0, -1);
                setCrumbs(next);
                void load(next[next.length - 1]?.path);
              }}
            >
              ←
            </button>
          )}
          <button style={{ ...css.btnGhost, height: 30 }} disabled={busy} onClick={() => void load()}>
            {L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose}>
            ×
          </button>
        </div>
      </div>
      <div style={{ maxHeight: 320, overflow: "auto", marginTop: 10 }}>
        {busy && !rows && <div style={css.muted}>{L.loadingApps}</div>}
        {rows?.map((r) => (
          <div
            key={r.path}
            style={{
              display: "flex",
              gap: 10,
              alignItems: "center",
              padding: "8px 10px",
              borderRadius: 8,
              border: "1px solid var(--border)",
              background: "var(--surface-2)",
              marginBottom: 6,
            }}
          >
            <div style={{ minWidth: 0, flex: 1 }}>
              <div style={{ fontWeight: 600 }}>{r.name}</div>
              <div style={{ ...css.muted, fontFamily: "var(--mono)", fontSize: 11.5 }}>
                {L.diskSizeApprox(formatSize(Math.max(0, r.size_kb)))} · {r.path}
              </div>
            </div>
            <button
              style={{ ...css.btnGhost, height: 26, padding: "0 8px" }}
              onClick={() => {
                setCrumbs((c) => [...c, r]);
                void load(r.path);
              }}
            >
              {L.detailShow}
            </button>
            <button
              style={{ ...css.btnGhost, height: 26, padding: "0 8px" }}
              onClick={() => void api.openPath(r.path)}
            >
              {L.openLocation}
            </button>
          </div>
        ))}
        {rows && rows.length === 0 && <div style={css.muted}>{L.orphanScanEmpty}</div>}
      </div>
    </div>
  );
}
