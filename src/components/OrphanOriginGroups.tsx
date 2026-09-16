import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import type { OriginGroup } from "../lib/decision";

/** Orphan results grouped by suspected folder origin (no invented product names). */
export function OrphanOriginGroups({
  groups,
  selectedPaths,
  onToggle,
}: {
  groups: OriginGroup[];
  selectedPaths: Set<string>;
  onToggle: (path: string) => void;
}) {
  const L = t();
  if (groups.length === 0) return null;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, marginBottom: 10 }}>
      {groups.map((g) => (
        <div
          key={g.origin}
          style={{
            ...css.card,
            padding: "10px 12px",
            fontSize: 12.5,
          }}
        >
          <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
            <strong style={{ fontSize: 13 }}>{g.origin}</strong>
            <span style={css.muted}>
              {L.orphanGroupMeta(g.count, g.safe, g.suggest, g.keep)}
            </span>
            {g.keep > 0 && (
              <span style={{ ...css.chip, fontFamily: "inherit", color: "var(--warn)" }}>
                {L.bucketKeep} {g.keep}
              </span>
            )}
          </div>
          <div style={{ marginTop: 6, display: "flex", flexDirection: "column", gap: 4 }}>
            {g.items.slice(0, 8).map((it) => (
              <label
                key={it.path}
                style={{
                  display: "flex",
                  gap: 8,
                  alignItems: "flex-start",
                  cursor: "pointer",
                  color: "var(--muted)",
                }}
              >
                <input
                  type="checkbox"
                  checked={selectedPaths.has(it.path)}
                  onChange={() => onToggle(it.path)}
                  style={{ marginTop: 2, accentColor: "var(--accent)" }}
                />
                <span style={{ minWidth: 0, flex: 1 }}>
                  <span className="ell" style={{ display: "block", fontFamily: "var(--mono)", fontSize: 11.5 }} title={it.path}>
                    {it.path}
                  </span>
                  <span style={{ fontSize: 11.5 }}>{L.orphanNoOwnerHint}</span>
                </span>
              </label>
            ))}
            {g.items.length > 8 && (
              <div style={css.muted}>… +{g.items.length - 8}</div>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}
