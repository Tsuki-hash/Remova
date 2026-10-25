import { useEffect, useState } from "react";
import { api, type DriveInfo } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { needsAdminRights, offerElevatedRestart } from "../lib/elevate";
import { toast } from "../lib/toast";
import { CloseGlyph } from "./ui/Glyph";
import type { ComponentStoreAnalysis, JunctionScanResult } from "../types";

const DEAD_LINKS_SHOWN = 50;

function gb(n: number): string {
  return `${n.toFixed(1)} GB`;
}

/**
 * Read-only disk-space overview: system volume shape, the junction ledger
 * (dead links called out — they also explain "why is this folder huge or
 * empty"), and an on-demand DISM component-store analysis. Nothing here
 * deletes, moves, or cleans.
 */
export function DiskOverviewPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [drives, setDrives] = useState<DriveInfo[] | null>(null);
  const [junction, setJunction] = useState<JunctionScanResult | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [dismBusy, setDismBusy] = useState(false);
  const [dism, setDism] = useState<ComponentStoreAnalysis | null>(null);

  useEffect(() => {
    let alive = true;
    setLoadError(null);
    void (async () => {
      try {
        const [d, j] = await Promise.all([api.listLocalDrives(), api.junctionLedger()]);
        if (!alive) return;
        setDrives(d);
        setJunction(j);
      } catch (e) {
        if (alive) setLoadError(formatError(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const runDism = async () => {
    if (dismBusy) return;
    setDismBusy(true);
    try {
      setDism(await api.analyzeComponentStore());
    } catch (e) {
      const msg = formatError(e);
      if (needsAdminRights(msg)) {
        await offerElevatedRestart(L.diskOverviewElevateHint);
      } else {
        toast.error(msg);
      }
    } finally {
      setDismBusy(false);
    }
  };

  const system = drives?.find((d) => d.is_system);
  const dead = junction?.entries.filter((e) => !e.reachable) ?? [];

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.diskOverviewTitle}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 8 }}>
        <strong>{L.diskOverviewTitle}</strong>
        <span style={css.muted}>{L.diskOverviewHint}</span>
        <button
          style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }}
          onClick={onClose}
          aria-label={L.panelClose}
        >
          <CloseGlyph />
        </button>
      </div>

      {loadError && (
        <div role="alert" style={{ color: "var(--danger-text)", marginBottom: 8 }}>
          {loadError}
        </div>
      )}

      {system && (
        <div style={css.muted}>
          {L.diskOverviewSystemVolume(
            system.letter,
            gb(system.total_gb - system.free_gb),
            gb(system.total_gb),
            gb(system.free_gb),
          )}
        </div>
      )}

      {junction && (
        <div style={{ marginTop: 10 }}>
          <div style={{ fontWeight: 600, marginBottom: 4 }}>
            {L.diskOverviewJunctionSummary(junction.entries.length, dead.length)}
          </div>
          {junction.degraded_roots.length > 0 && (
            <div style={{ ...css.muted, fontSize: 12 }}>
              {L.diskOverviewDegraded(junction.degraded_roots.join("、"))}
            </div>
          )}
          {dead.length > 0 && (
            <div
              role="note"
              style={{
                marginTop: 6,
                border: "1px solid var(--warn-ink)",
                borderRadius: 8,
                padding: "6px 10px",
                fontSize: 12,
              }}
            >
              {dead.slice(0, DEAD_LINKS_SHOWN).map((e) => (
                <div key={e.path} className="ell" style={{ color: "var(--warn-ink)" }}>
                  {L.diskOverviewDeadLink(e.path, e.target ?? "?")}
                </div>
              ))}
              {dead.length > DEAD_LINKS_SHOWN && (
                <div style={css.muted}>{L.diskOverviewOmitted(dead.length - DEAD_LINKS_SHOWN)}</div>
              )}
            </div>
          )}
          {junction.omitted > 0 && (
            <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>
              {L.scanOmitted(junction.omitted)}
            </div>
          )}
        </div>
      )}

      <div style={{ marginTop: 10, borderTop: "1px solid var(--border)", paddingTop: 8 }}>
        <div style={{ fontWeight: 600 }}>{L.diskOverviewWinSxs}</div>
        <div style={{ ...css.muted, fontSize: 12, margin: "4px 0 6px" }}>{L.diskOverviewWinSxsHint}</div>
        {dism && (
          <div role="status" style={{ marginBottom: 6 }}>
            {L.diskOverviewWinSxsResult(
              dism.actual_size_bytes !== null ? gb(dism.actual_size_bytes / 1024 ** 3) : "?",
              dism.explorer_size_bytes !== null ? gb(dism.explorer_size_bytes / 1024 ** 3) : "?",
              dism.cleanup_recommended ? L.diskOverviewRecommendedYes : L.diskOverviewRecommendedNo,
            )}
          </div>
        )}
        <button
          type="button"
          style={{ ...css.btnGhost, height: 30 }}
          disabled={dismBusy}
          onClick={() => void runDism()}
        >
          {dismBusy ? L.diskOverviewAnalyzing : L.diskOverviewAnalyze}
        </button>
      </div>

      <div style={{ ...css.muted, fontSize: 12, marginTop: 10 }}>{L.diskOverviewScopeNote}</div>
    </section>
  );
}
