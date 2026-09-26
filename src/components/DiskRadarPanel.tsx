import { useEffect, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { VirtualList } from "./ui/VirtualList";
import { api, type DriveInfo } from "../lib/api";
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
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [drive, setDrive] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [crumbs, setCrumbs] = useState<DirSizeRow[]>([]);

  const load = async (parent?: string, letter?: string) => {
    setBusy(true);
    try {
      const list = parent
        ? await api.listDirChildren(parent)
        : await api.listTopDirSizes(letter);
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
        const list = await api.listLocalDrives();
        setDrives(list);
        const sys = list.find((d) => d.is_system) ?? list[0];
        const letter = sys?.letter ?? "";
        setDrive(letter);
        await load(undefined, letter);
      } catch (e) {
        onError(formatError(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const switchDrive = async (letter: string) => {
    setDrive(letter);
    setCrumbs([]);
    await load(undefined, letter);
  };

  const current = drives.find((d) => d.letter === drive);

  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
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
              onClick={() => {
                const next = crumbs.slice(0, -1);
                setCrumbs(next);
                void load(next[next.length - 1]?.path, drive);
              }}
            >
              ←
            </button>
          )}
          <button
            style={{ ...css.btnGhost, height: 30 }}
            disabled={busy}
            onClick={() => void load(undefined, drive)}
          >
            {L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>
      {busy && !rows && <div style={{ ...css.muted, marginTop: 10 }}>{L.loadingApps}</div>}
      <VirtualList
        items={rows ?? []}
        height={320}
        estimateSize={72}
        empty={<div style={css.muted}>{L.orphanScanEmpty}</div>}
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
        )}
      />
    </div>
  );
}
