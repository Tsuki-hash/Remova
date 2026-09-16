import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type CheckupStats = {
  total: number;
  large: number;
  recent: number;
};

/** Full software checkup panel (counts only; no fake health score). */
export function CheckupPanel({
  stats,
  scanning,
  orphanCount,
  onClose,
  onOrphanScan,
  onOpenOrphans,
}: {
  stats: CheckupStats;
  scanning: boolean;
  orphanCount: number | null;
  onClose: () => void;
  onOrphanScan: () => void;
  onOpenOrphans: () => void;
}) {
  const L = t();
  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 8500,
        background: "rgba(0,0,0,.35)",
        display: "grid",
        placeItems: "center",
        padding: 16,
      }}
      role="dialog"
      aria-modal="true"
    >
      <div
        style={{
          width: "min(480px, 96vw)",
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: 14,
          boxShadow: "var(--shadow)",
          padding: 16,
          fontSize: 13,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <strong style={{ fontSize: 15 }}>{L.checkupTitle}</strong>
          <button style={{ ...css.btnGhost, marginLeft: "auto", height: 28 }} onClick={onClose}>
            ×
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
              value:
                orphanCount === null
                  ? scanning
                    ? "…"
                    : "—"
                  : String(orphanCount),
              ok: orphanCount !== null,
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
                {row.ok ? "✓" : "○"}
              </span>
              <span style={{ flex: 1 }}>{row.label}</span>
              <strong style={{ fontFamily: "var(--mono)" }}>{row.value}</strong>
            </div>
          ))}
        </div>
        {orphanCount !== null && orphanCount > 0 && (
          <div
            style={{
              marginTop: 10,
              padding: "8px 10px",
              borderRadius: 8,
              background: "var(--accent-soft)",
              fontSize: 12.5,
            }}
          >
            {L.checkupOrphanFound(orphanCount)}
          </div>
        )}
        <div style={{ display: "flex", gap: 8, marginTop: 14, justifyContent: "flex-end" }}>
          <button style={css.btnGhost} disabled={scanning} onClick={onOrphanScan}>
            {scanning ? L.orphanScanning : L.checkupRunOrphan}
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
