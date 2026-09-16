import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type CategoryId = "all" | "desktop" | "store" | "large" | "recent";

/** Beginner toolbar: search + 3 plain filters (store/desktop live in meta only). */
export function SoftwareToolbar({
  q,
  category,
  estimating,
  scanning,
  onQuery,
  onCategory,
  onStopEstimate,
}: {
  q: string;
  category: CategoryId;
  estimating: boolean;
  scanning: boolean;
  onQuery: (v: string) => void;
  onCategory: (id: CategoryId) => void;
  onStopEstimate: () => void;
}) {
  const L = t();
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "nowrap",
        gap: 10,
        alignItems: "center",
        marginBottom: 10,
        flexShrink: 0,
      }}
    >
      <input
        style={{ ...css.input, flex: "1 1 auto", minWidth: 160, height: 34 }}
        placeholder={L.search}
        value={q}
        onChange={(e) => onQuery(e.target.value)}
        aria-label={L.search}
      />
      <div style={{ display: "flex", gap: 4, flexShrink: 0 }}>
        {(
          [
            ["all", L.catAll],
            ["large", L.catLarge],
            ["recent", L.catRecent],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            style={{
              height: 32,
              padding: "0 14px",
              borderRadius: 8,
              border: "1px solid var(--border)",
              background: category === id ? "var(--accent-soft)" : "var(--surface)",
              color: category === id ? "var(--accent)" : "var(--fg)",
              fontWeight: category === id ? 650 : 500,
              fontSize: 12.5,
              cursor: "pointer",
            }}
            onClick={() => onCategory(id)}
          >
            {label}
          </button>
        ))}
      </div>
      <span
        title={L.guided}
        style={{
          ...css.muted,
          width: 22,
          height: 22,
          borderRadius: 999,
          border: "1px solid var(--border)",
          display: "grid",
          placeItems: "center",
          cursor: "help",
          flexShrink: 0,
        }}
      >
        ⓘ
      </span>
      {estimating && (
        <button style={{ ...css.btnSm, height: 32 }} title={L.stopEstimateHint} onClick={onStopEstimate}>
          {L.stopEstimate}
        </button>
      )}
      {scanning && <span style={{ ...css.muted, flexShrink: 0 }}>{L.analyzing}</span>}
    </div>
  );
}
