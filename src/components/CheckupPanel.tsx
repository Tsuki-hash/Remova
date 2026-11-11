import { useRef, type ReactNode } from "react";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { dialogLayer, useDialogFocus } from "../lib/useDialogFocus";
import { LEGACY_SCAN } from "../lib/scopedScan";
import { ScanCompletenessNotice } from "./ScanCompletenessNotice";
import { CloseGlyph, Deco } from "./ui/Glyph";

export type CheckupStats = {
  total: number;
  large: number;
  recent: number;
};

/** Full software checkup panel (counts only; no fake health score). */
export function CheckupPanel({
  stats,
  orphanScanning,
  scanStatus,
  orphanCount,
  orphanCompleteness,
  onClose,
  onOrphanScan,
  onOpenOrphans,
}: {
  stats: CheckupStats;
  /** Orphan scan spinner is independent of the analyze spinner. */
  orphanScanning: boolean;
  scanStatus?: ReactNode;
  orphanCount: number | null;
  /** Completeness of the scan behind `orphanCount`; partial counts are not totals. */
  orphanCompleteness?: import("../types").ScanCompleteness | null;
  onClose: () => void;
  onOrphanScan: () => void;
  onOpenOrphans: () => void;
}) {
  const L = t();
  const completeness = orphanCompleteness ?? LEGACY_SCAN;
  const orphanComplete = orphanCount !== null && completeness.status === "complete";
  const orphanValue = orphanCount === null
    ? orphanScanning ? "…" : "—"
    : orphanComplete ? String(orphanCount)
      : completeness.status === "partial" ? `${orphanCount}+` : "—";
  const dialogRef = useRef<HTMLDivElement>(null);
  useDialogFocus(true, dialogRef, dialogLayer.overview, onClose);
  return (
    <div
      style={{
        position: "fixed",
        animation: "remova-fade-in var(--dur-float) var(--ease-standard)",
        inset: 0,
        zIndex: dialogLayer.overview,
        background: "rgba(0,0,0,.35)",
        display: "grid",
        placeItems: "center",
        padding: 16,
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={L.checkupTitle}
        tabIndex={-1}
        ref={dialogRef}
        style={{
          outline: "none",
          width: "min(480px, 96vw)",
          maxHeight: "calc(100dvh - 32px)",
          overflow: "auto",
          overflowWrap: "anywhere",
          background: "var(--surface-elevated)",
          border: "1px solid var(--border)",
          borderRadius: 14,
          boxShadow: "var(--shadow-float)",
          padding: 16,
          fontSize: 13,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <strong style={{ fontSize: 15 }}>{L.checkupTitle}</strong>
          <button style={{ ...css.btnGhost, marginLeft: "auto", height: 28 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
        <div style={{ ...css.muted, marginTop: 6, lineHeight: 1.5 }}>{L.checkupFlowHint}</div>
        <div style={{ display: "grid", gap: 8, marginTop: 12 }}>
          {[
            { label: L.checkupStepApps, value: String(stats.total), ok: true },
            { label: L.checkupStepLarge, value: String(stats.large), ok: true },
            { label: L.checkupStepRecent, value: String(stats.recent), ok: true },
            {
              label: L.checkupStepOrphan,
              value: orphanValue,
              ok: orphanComplete,
            },
          ].map((row) => (
            <div
              key={row.label}
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
              <span style={{ color: row.ok ? "var(--ok)" : "var(--muted)", fontWeight: 700 }}>
                {row.ok ? <Deco ch="✓" /> : <Deco ch="○" />}
              </span>
              <span style={{ flex: 1 }}>{row.label}</span>
              <strong style={{ fontFamily: "var(--mono)" }}>{row.value}</strong>
            </div>
          ))}
        </div>
        {scanStatus}
        {orphanCount !== null && <ScanCompletenessNotice completeness={completeness} />}
        {orphanComplete && orphanCount > 0 && (
          <div
            style={{
              marginTop: 10,
              padding: "8px 10px",
              borderRadius: 8,
              background: "var(--accent-soft)",
              fontSize: 13,
            }}
          >
            {L.checkupOrphanFound(orphanCount)}
          </div>
        )}
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: 14, justifyContent: "flex-end" }}>
          <button style={css.btnGhost} disabled={orphanScanning} onClick={onOrphanScan}>
            {orphanScanning ? L.orphanScanning : L.checkupRunOrphan}
          </button>
          <button
            style={{ ...css.btn, opacity: orphanCount && orphanCount > 0 ? 1 : 0.5 }}
            disabled={!orphanCount}
            onClick={onOpenOrphans}
          >
            {L.navOrphans}
          </button>
        </div>
      </div>
    </div>
  );
}
