import { useEffect, useRef, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { VirtualList } from "./ui/VirtualList";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendText } from "../lib/backendText";
import type { IdleApp } from "../types";
import { useAssociationScan, scanCancelled } from "../hooks/useAssociationScan";
import { ScanProgressBar } from "./ScanProgressBar";

/** Idle software radar — read-only ranking; jump back to the software list to uninstall. */
export function IdleRadarPanel({
  onClose,
  onGoSoftware,
  onError,
}: {
  onClose: () => void;
  onGoSoftware: () => void;
  onError: (msg: string) => void;
}) {
  const L = t();
  const [rows, setRows] = useState<IdleApp[] | null>(null);
  const [busy, setBusy] = useState(false);
  const { runItems, cancel, progress } = useAssociationScan();
  const scanRound = useRef(0);

  const load = async () => {
    const round = ++scanRound.current;
    setBusy(true);
    setRows(null);
    try {
      setRows(await runItems(api.rankIdleApps));
    } catch (e) {
      if (round === scanRound.current && !scanCancelled(e)) onError(formatError(e));
    } finally {
      if (round === scanRound.current) setBusy(false);
    }
  };

  useEffect(() => {
    void load();
    return () => { scanRound.current++; };
 // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <strong>{L.idleTitle}</strong>
        <span style={css.muted}>{L.idleHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button style={{ ...css.btnGhost, height: 30 }} disabled={busy} title={busy ? L.idleScanning : undefined} onClick={() => void load()}>
            {L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>
      <ScanProgressBar progress={progress} onCancel={cancel} />
      <VirtualList
        items={rows ?? []}
        height={320}
        estimateSize={72}
        listRole
        empty={rows === null ? undefined : <div style={css.muted}>{L.idleEmpty}</div>}
        keyOf={(r) => r.app.registry_key || r.app.name}
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
              <div style={{ fontWeight: 600 }}>{r.app.name}</div>
              <div style={{ ...css.muted, fontSize: 11.5, fontFamily: "var(--mono)" }}>
                {L.idleDays(r.idle_days)} · {r.evidence.some(e => e.code === "idle_size_partial") ? "≥" : ""}{formatSize(Math.max(0, r.size_kb))}
                {r.app.install_date ? ` · ${r.app.install_date}` : ""}
              </div>
              <div style={{ ...css.muted, fontSize: 11 }}>
                {r.evidence
                  .map((e) => backendText(e.code, "idle"))
                  .join(" · ")}
              </div>
            </div>
            <button style={{ ...css.btnGhost, height: 28 }} onClick={onGoSoftware}>
              {L.goToSoftware}
            </button>
          </div>
        )}
      />
    </div>
  );
}
