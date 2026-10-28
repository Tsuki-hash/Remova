import { useEffect, useRef, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { VirtualList } from "./ui/VirtualList";
import { api, type DriveInfo } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import type { DirSizeRow } from "../types";
import { useAssociationScan, scanCancelled } from "../hooks/useAssociationScan";
import { useRequestSeq } from "../hooks/useRequestSeq";
import { ScanProgressBar } from "./ScanProgressBar";

/** Off-screen text for the aggregate badge's description; never painted. */
const visuallyHidden = {
  position: "absolute",
  width: 1,
  height: 1,
  margin: -1,
  padding: 0,
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
  border: 0,
} as const;

/** What the current list does not show (drill-down only). */
type PartialInfo = { omitted: number; capped: boolean };

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
  const [partial, setPartial] = useState<PartialInfo | null>(null);
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [drive, setDrive] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [crumbs, setCrumbs] = useState<DirSizeRow[]>([]);
  const { next: nextRequest, isCurrent: isCurrentRequest, invalidate: invalidateRequest } = useRequestSeq();
  const busyRef = useRef(false);
  const { runReadOnly, cancel, progress } = useAssociationScan();

  const load = async (parent?: string, letter?: string, nextCrumbs: DirSizeRow[] = []) => {
    if (busyRef.current) return;
    busyRef.current = true;
    const seq = nextRequest();
    setBusy(true);
    try {
      if (parent) {
        // The drill-down result carries what the list leaves out (omitted
        // children, a cut parent scan); the top view says it through the
        // per-row floor markers instead.
        const result = await runReadOnly(
          id => api.listDirChildren(parent, id),
          r => (r?.rows.length ?? 0),
        );
        if (isCurrentRequest(seq)) {
          setRows(result?.rows ?? []);
          setPartial({ omitted: result?.omitted ?? 0, capped: result?.capped ?? false });
          setCrumbs(nextCrumbs);
          setDrive(drive);
        }
      } else {
        const list = await runReadOnly(
          id => api.listTopDirSizes(letter, id),
          r => r.length,
        );
        if (isCurrentRequest(seq)) {
          setRows(list);
          setPartial(null);
          setCrumbs(nextCrumbs);
          if (letter !== undefined) setDrive(letter);
        }
      }
    } catch (e) {
      if (isCurrentRequest(seq)) {
        // A cancel must not throw away what the user was already looking at;
        // the progress bar says the scan was stopped.
        if (!scanCancelled(e)) onError(formatError(e));
      }
    } finally {
      if (isCurrentRequest(seq)) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const list = await api.listLocalDrives();
        if (!alive) return;
        setDrives(list);
        const sys = list.find((d) => d.is_system) ?? list[0];
        const letter = sys?.letter ?? "";
        await load(undefined, letter);
      } catch (e) {
        if (alive) onError(formatError(e));
      }
    })();
    return () => { alive = false; invalidateRequest(); busyRef.current = false; };
 // Mount-only: the first listing starts once; a later prop change must not restart it.
 // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const switchDrive = async (letter: string) => {
    await load(undefined, letter);
  };

  const current = drives.find((d) => d.letter === drive);
  const listEmpty = (rows ?? []).length === 0;

  return (
    <div style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <strong>{L.diskRadarTitle}</strong>
        <span style={css.muted}>{L.diskRadarHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
          {drives.map((d) => (
            <button
              key={d.letter}
              type="button"
              style={{
                ...css.btnGhost,
                height: 28,
                padding: "0 10px",
                fontWeight: drive === d.letter ? 700 : 500,
                borderColor: drive === d.letter ? "var(--accent)" : undefined,
              }}
              disabled={busy}
              aria-pressed={drive === d.letter}
              title={`${formatSize(Math.round(d.free_gb * 1024 * 1024))} / ${formatSize(Math.round(d.total_gb * 1024 * 1024))}`}
              onClick={() => void switchDrive(d.letter)}
            >
              {d.letter}:
            </button>
          ))}
          {current && (
            <span style={{ ...css.muted, fontFamily: "var(--mono)", fontSize: 11.5 }}>
              {formatSize(Math.round(current.free_gb * 1024 * 1024))} /{" "}
              {formatSize(Math.round(current.total_gb * 1024 * 1024))}
            </span>
          )}
          {crumbs.length > 0 && (
            <button
              style={{ ...css.btnGhost, height: 30 }}
              aria-label={L.navBack}
              disabled={busy}
              onClick={() => {
                const next = crumbs.slice(0, -1);
                void load(next[next.length - 1]?.path, drive, next);
              }}
            >
              ←
            </button>
          )}
          <button
            style={{ ...css.btnGhost, height: 30 }}
            disabled={busy}
            onClick={() => void load(undefined, drive)}
            title={busy ? L.radarScanning : undefined}
          >
            {L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>
      <ScanProgressBar progress={progress} onCancel={cancel} cancelledLabel={L.diskRadarCancelled} />
      <VirtualList
        items={rows ?? []}
        height={320}
        estimateSize={72}
        listRole
        ariaLabel={L.diskRadarTitle}
        empty={
          rows === null ? undefined : (
            <div style={css.muted}>{partial?.capped ? L.diskRadarPartial : L.radarEmpty}</div>
          )
        }
        keyOf={(r) => r.path}
        renderItem={(r) => (
          <div
            style={{
              display: "flex",
              gap: 10,
              alignItems: "center",
              padding: "8px 10px",
              borderRadius: 8,
              border: "1px solid var(--border)",
              background: "var(--surface-2)",
            }}
          >
            <div style={{ minWidth: 0, flex: 1 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
                <span className="ell" style={{ fontWeight: 600 }}>
                  {r.name}
                </span>
                {r.aggregate && (
                  <span
                    aria-describedby={`radar-agg-${r.path}`}
                    style={{
                      flexShrink: 0,
                      fontSize: 10.5,
                      fontWeight: 600,
                      padding: "1px 6px",
                      borderRadius: 999,
                      border: "1px solid var(--border)",
                      color: "var(--muted)",
                    }}
                  >
                    {L.diskRadarAggregate}
                    <span id={`radar-agg-${r.path}`} style={visuallyHidden}>
                      {L.diskRadarAggregateHint}
                    </span>
                  </span>
                )}
              </div>
              <div
                title={r.path}
                style={{
                  ...css.muted,
                  fontFamily: "var(--mono)",
                  fontSize: 11.5,
                  // Long Windows paths must wrap instead of pushing the two
                  // action buttons out of the row.
                  overflowWrap: "anywhere",
                }}
              >
                {r.capped
                  ? L.diskRadarFloor(r.size_kb > 0 ? formatSize(r.size_kb) : "0 KB", r.scanned)
                  : L.diskSizeApprox(formatSize(Math.max(0, r.size_kb)))} · {r.path}
              </div>
            </div>
            <button
              style={{ ...css.btnGhost, height: 26, padding: "0 8px", flexShrink: 0 }}
              disabled={busy}
              onClick={() => {
                void load(r.path, drive, [...crumbs, r]);
              }}
            >
              {L.detailShow}
            </button>
            <button
              style={{ ...css.btnGhost, height: 26, padding: "0 8px", flexShrink: 0 }}
              onClick={() => void api.openPath(r.path).catch((e) => onError(formatError(e)))}
            >
              {L.openLocation}
            </button>
          </div>
        )}
      />
      {!listEmpty && (partial?.omitted ?? 0) > 0 && (
        <div style={{ ...css.muted, fontSize: 12, marginTop: 6 }}>{L.scanOmitted(partial!.omitted)}</div>
      )}
      {!listEmpty && partial?.capped && (
        <div style={{ ...css.muted, fontSize: 12, marginTop: 2 }}>{L.diskRadarPartial}</div>
      )}
    </div>
  );
}
