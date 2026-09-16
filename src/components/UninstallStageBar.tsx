import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type UninstallStage = "idle" | "identify" | "official" | "scan" | "report";

type ActiveStage = Exclude<UninstallStage, "idle">;

const ORDER: ActiveStage[] = ["identify", "official", "scan", "report"];

/** Visible four-stage deep-uninstall progress (report §5). */
export function UninstallStageBar({
  stage,
  detail,
}: {
  stage: UninstallStage;
  detail?: string;
}) {
  const L = t();
  if (stage === "idle") return null;
  const labels: Record<ActiveStage, string> = {
    identify: L.stageIdentify,
    official: L.stageOfficial,
    scan: L.stageScanLeftover,
    report: L.stageDoneReport,
  };
  const idx = ORDER.indexOf(stage as ActiveStage);
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: 8,
        alignItems: "center",
        padding: "8px 14px",
        marginBottom: 8,
        borderRadius: 10,
        border: "1px solid var(--border)",
        background: "var(--surface-2)",
        fontSize: 12,
        flexShrink: 0,
      }}
      role="status"
      aria-live="polite"
    >
      <strong style={{ fontSize: 12.5 }}>{L.uninstallFlowDeep}</strong>
      {ORDER.map((s, i) => {
        const done = i < idx;
        const active = s === stage;
        return (
          <span key={s} style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <span
              style={{
                width: 18,
                height: 18,
                borderRadius: 999,
                display: "grid",
                placeItems: "center",
                fontSize: 10,
                fontWeight: 700,
                background: done || active ? "var(--accent)" : "var(--surface)",
                color: done || active ? "var(--accent-ink)" : "var(--muted)",
                border: "1px solid var(--border)",
              }}
            >
              {done ? "✓" : i + 1}
            </span>
            <span
              style={{
                color: active ? "var(--fg)" : "var(--muted)",
                fontWeight: active ? 650 : 500,
              }}
            >
              {labels[s]}
            </span>
            {i < ORDER.length - 1 && (
              <span style={{ color: "var(--muted)", opacity: 0.5 }} aria-hidden>
                →
              </span>
            )}
          </span>
        );
      })}
      {stage === "scan" && (
        <div className="remova-progress" style={{ flex: "1 1 80px", minWidth: 60 }} aria-hidden />
      )}
      {detail && <span style={{ ...css.muted, flex: "1 1 120px" }}>{detail}</span>}
    </div>
  );
}
