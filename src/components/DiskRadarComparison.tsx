import { useState } from "react";
import { currentLang, formatSize, t } from "../i18n";
import {
  compareDiskRadar, DISK_RADAR_SNAPSHOT_KEY, readDiskRadarSnapshot, snapshotDiskRadar,
} from "../lib/diskRadarSnapshot";
import type { DirSizeRow } from "../types";
import { cssStyles as css } from "../styles";

export type RadarScope = { volume: string; range: string };

export function DiskRadarComparison({ scope, rows, busy, onInspect }: {
  scope: RadarScope | null;
  rows: DirSizeRow[] | null;
  busy: boolean;
  onInspect?: (row: DirSizeRow) => void;
}) {
  const L = t();
  const [saved, setSaved] = useState(() => {
    try {
      return { snapshot: readDiskRadarSnapshot(localStorage), failed: false };
    } catch {
      return { snapshot: null, failed: true };
    }
  });
  const snapshot = saved.snapshot;
  if (!rows && !snapshot && !saved.failed) return null;
  const changes = snapshot && scope && rows ? compareDiskRadar(snapshot, scope.volume, scope.range, rows) : null;
  const remember = () => {
    if (!scope || !rows || busy) return;
    try {
      const next = snapshotDiskRadar(scope.volume, scope.range, rows);
      localStorage.setItem(DISK_RADAR_SNAPSHOT_KEY, JSON.stringify(next));
      setSaved({ snapshot: next, failed: false });
    } catch {
      setSaved(current => ({ ...current, failed: true }));
    }
  };
  const clear = () => {
    try {
      localStorage.removeItem(DISK_RADAR_SNAPSHOT_KEY);
      setSaved({ snapshot: null, failed: false });
    } catch {
      setSaved(current => ({ ...current, failed: true }));
    }
  };
  return (
    <section aria-label={L.diskRadarCompareTitle} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 12, marginBottom: 10 }}>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <strong style={{ flex: 1 }}>{L.diskRadarCompareTitle}</strong>
        <button type="button" style={css.btnGhost} onClick={remember} disabled={busy || !scope || !rows}>
          {snapshot ? L.diskRadarReplaceSnapshot : L.diskRadarSaveSnapshot}
        </button>
        {(snapshot || saved.failed) && <button type="button" style={css.btnGhost} onClick={clear}>{L.diskRadarClearSnapshot}</button>}
      </div>
      {saved.failed && <p role="status">{L.diskRadarSnapshotError}</p>}
      {snapshot && <p style={css.muted}>{L.diskRadarSavedAt(new Date(snapshot.takenAt).toLocaleString(currentLang() === "zh" ? "zh-CN" : "en"))}</p>}
      {!busy && rows && !scope && <p style={css.muted}>{L.diskRadarScopeUnknown}</p>}
      {!busy && snapshot && scope && rows && !changes && <p role="status">{L.diskRadarScopeMismatch}</p>}
      {changes && <details style={{ marginTop: 6 }}>
        <summary>{L.diskRadarCompareDetails(changes.filter(change => change.delta !== undefined).length)}</summary>
        <ul style={{ listStyle: "none", margin: "8px 0 0", padding: 0, maxHeight: 180, overflowY: "auto" }}>
          {changes.map(change => <li key={change.key}
            style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center", borderTop: "1px solid var(--border)", padding: "6px 0" }}>
            <span style={{ flex: "1 1 200px", overflowWrap: "anywhere" }}>{change.name}</span>
            <span style={{ ...css.muted, fontFamily: "var(--mono)" }}>{change.delta === undefined
              ? change.before === undefined ? L.diskRadarNewRow : change.after === undefined ? L.diskRadarMissingRow : L.diskRadarCappedRow
              : change.delta === 0 ? L.diskRadarUnchanged : `${change.delta > 0 ? "+" : "−"}${formatSize(Math.abs(change.delta))}`}</span>
            {change.delta !== undefined && change.delta > 0 && change.row && onInspect && (
              <button type="button" style={css.btnGhost} onClick={() => onInspect(change.row!)}>{L.folderInspectAction}</button>
            )}
          </li>)}
        </ul>
      </details>}
    </section>
  );
}
