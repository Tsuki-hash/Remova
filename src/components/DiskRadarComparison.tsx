import { useId, useState } from "react";
import { currentLang, formatSize, t } from "../i18n";
import {
  compareDiskRadar, DISK_RADAR_SNAPSHOT_KEY, readDiskRadarSnapshot, snapshotDiskRadar,
  type DiskRadarChange,
} from "../lib/diskRadarSnapshot";
import type { DirSizeRow } from "../types";
import { cssStyles as css } from "../styles";
import "./DiskRadarComparison.css";

export type RadarScope = { volume: string; range: string };

type GrowingDirectory = DiskRadarChange & { before: number; after: number; delta: number; row: DirSizeRow };
const GROWTH_LIMIT = 5;

function grew(change: DiskRadarChange): change is GrowingDirectory {
  return Boolean(change.row && !change.row.aggregate && change.before !== undefined &&
    change.after !== undefined && change.delta !== undefined && change.delta > 0);
}

function measuredSize(size: number) {
  return size === 0 ? "0 KB" : formatSize(size);
}

function scanTime(time: number) {
  return new Date(time).toLocaleString(currentLang() === "zh" ? "zh-CN" : "en", {
    dateStyle: "short", timeStyle: "medium",
  });
}

export function DiskRadarComparison({ scope, rows, measuredAt, busy, stale, onInspect }: {
  scope: RadarScope | null;
  rows: DirSizeRow[] | null;
  measuredAt: number | null;
  busy: boolean;
  stale: boolean;
  onInspect?: (row: DirSizeRow, opener: HTMLButtonElement) => void;
}) {
  const L = t();
  const titleId = useId();
  const growthId = useId();
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
  const top = changes?.filter(grew).slice(0, GROWTH_LIMIT) ?? [];
  const other = changes?.filter(change => !top.some(item => item.key === change.key)) ?? [];
  const previous = busy || stale;
  const remember = () => {
    if (!scope || !rows || measuredAt === null || previous) return;
    try {
      const next = snapshotDiskRadar(scope.volume, scope.range, rows, measuredAt);
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
  const inspectButton = (row: DirSizeRow) => onInspect && (
    <button type="button" style={css.btnGhost} disabled={busy}
      aria-label={`${L.diskRadarInspectGrowth}: ${row.path}`}
      onClick={event => onInspect(row, event.currentTarget)}>{L.diskRadarInspectGrowth}</button>
  );
  return (
    <section className="disk-radar-comparison" aria-labelledby={titleId}>
      <header className="disk-radar-comparison-heading">
        <h3 id={titleId}>{L.diskRadarCompareTitle}</h3>
        <button type="button" style={css.btnGhost} onClick={remember} disabled={previous || !scope || !rows || measuredAt === null}>
          {snapshot ? L.diskRadarReplaceSnapshot : L.diskRadarSaveSnapshot}
        </button>
        {(snapshot || saved.failed) && <button type="button" style={css.btnGhost} onClick={clear}>{L.diskRadarClearSnapshot}</button>}
      </header>
      {saved.failed && <p role="status">{L.diskRadarSnapshotError}</p>}
      {!snapshot && <p className="disk-radar-comparison-note">{L.diskRadarNeedBaseline}</p>}
      {snapshot && <p className="disk-radar-comparison-note">{L.diskRadarSavedAt(scanTime(snapshot.takenAt))}</p>}
      {measuredAt !== null && <p className="disk-radar-comparison-note">
        {L.diskRadarMeasuredAt(scanTime(measuredAt))} {previous && L.diskRadarShowingPrevious}
      </p>}
      {!busy && rows && !scope && <p className="disk-radar-comparison-note">{L.diskRadarScopeUnknown}</p>}
      {!busy && snapshot && scope && rows && !changes && <p role="status">{L.diskRadarScopeMismatch}</p>}
      {changes && top.length === 0 && <p role="status">{L.diskRadarNoGrowth}</p>}
      {top.length > 0 && <>
        <strong id={growthId}>{L.diskRadarLargestGrowth}</strong>
        <p className="disk-radar-comparison-note">{L.diskRadarGrowthNotSavings}</p>
        <ol className="disk-radar-comparison-list" aria-labelledby={growthId}>
          {top.map(change => (
            <li className="disk-radar-comparison-row" key={change.key}>
              <div className="disk-radar-comparison-folder">
                <strong>{change.name}</strong>
                <span className="disk-radar-comparison-path">{change.row.path}</span>
              </div>
              <div className="disk-radar-comparison-size">
                <strong>+{formatSize(change.delta)}</strong>
                <div className="disk-radar-comparison-path">{L.diskRadarGrowthSizes(measuredSize(change.before), measuredSize(change.after))}</div>
              </div>
              {inspectButton(change.row)}
            </li>
          ))}
        </ol>
      </>}
      {other.length > 0 && <details>
        <summary>{L.diskRadarCompareDetails(other.length)}</summary>
        <ul className="disk-radar-comparison-list">
          {other.map(change => <li className="disk-radar-comparison-row" key={change.key}>
            <span className="disk-radar-comparison-folder">{change.name}</span>
            <span className="disk-radar-comparison-size">{change.delta === undefined
              ? change.before === undefined ? L.diskRadarNewRow : change.after === undefined ? L.diskRadarMissingRow : L.diskRadarCappedRow
              : change.delta === 0 ? L.diskRadarUnchanged : `${change.delta > 0 ? "+" : "−"}${formatSize(Math.abs(change.delta))}`}</span>
            {grew(change) && inspectButton(change.row)}
          </li>)}
        </ul>
      </details>}
    </section>
  );
}
