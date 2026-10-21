import { useEffect, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { VirtualList } from "./ui/VirtualList";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendText } from "../lib/backendText";
import type { IdleApp } from "../types";
import { useAssociationScan, scanCancelled } from "../hooks/useAssociationScan";
import { useRequestSeq } from "../hooks/useRequestSeq";
import { ScanProgressBar } from "./ScanProgressBar";
import { normalizeIdleScan } from "../lib/scopedScan";

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
  const [omitted, setOmitted] = useState(0);
  const [busy, setBusy] = useState(false);
  const { runReadOnly, cancel, progress } = useAssociationScan();
  const { next: nextRound, isCurrent: isCurrentRound, invalidate: invalidateRound } = useRequestSeq();

  const load = async () => {
    const round = nextRound();
    setBusy(true);
    setRows(null);
    setOmitted(0);
    try {
      const { items, omitted } = normalizeIdleScan(
        await runReadOnly(
          (id: number) => api.rankIdleApps(id),
          (r) => normalizeIdleScan(r).items.length,
        ),
      );
      setRows(items);
      setOmitted(omitted);
    } catch (e) {
      if (isCurrentRound(round) && !scanCancelled(e)) onError(formatError(e));
    } finally {
      if (isCurrentRound(round)) setBusy(false);
    }
  };

  useEffect(() => {
    void load();
    // Unmounting retires the in-flight round so a late ranking cannot publish.
    return invalidateRound;
 // Mount-only: the ranking starts once; a later prop change must not restart it.
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
      {omitted > 0 && (
        <div style={{ ...css.muted, marginTop: 6, fontSize: 12 }} role="status">
          {L.scanOmitted(omitted)}
        </div>
      )}
      <VirtualList
        items={rows ?? []}
        height={320}
        estimateSize={72}
        listRole
        ariaLabel={L.idleTitle}
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
              </div>
              <div style={{ ...css.muted, fontSize: 11 }}>
                {r.evidence.map((e, i) => (
                  <span key={`${e.code}-${i}`}>
                    {i > 0 ? " · " : ""}
                    {/* The date belongs to its label; keep the pair on one line. */}
                    {e.code === "idle_install_age" && r.app.install_date ? (
                      <span style={{ whiteSpace: "nowrap" }}>
                        {backendText(e.code, "idle")} {r.app.install_date}
                      </span>
                    ) : (
                      backendText(e.code, "idle")
                    )}
                  </span>
                ))}
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
