import { useState } from "react";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import type { OriginGroup } from "../lib/decision";
import { bucketItem } from "../lib/decision";
import type { CleanupItem } from "../types";

function riskChip(it: CleanupItem, L: ReturnType<typeof t>) {
  const bucket = bucketItem(it);
  const label =
    bucket === "safe" ? L.bucketSafe : bucket === "suggest" ? L.bucketSuggest : L.bucketKeep;
  const color =
    bucket === "safe" ? "var(--ok)" : bucket === "suggest" ? "var(--warn)" : "var(--danger)";
  return { label, color, bucket };
}

/** Orphan results grouped by suspected folder origin (no invented product names). */
export function OrphanOriginGroups({
  groups,
  selectedPaths,
  onToggle,
  onToggleMany,
}: {
  groups: OriginGroup[];
  selectedPaths: Set<string>;
  onToggle: (path: string) => void;
  onToggleMany?: (paths: string[], select: boolean) => void;
}) {
  const L = t();
  const [openPath, setOpenPath] = useState<string | null>(null);
  if (groups.length === 0) return null;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, marginBottom: 10 }}>
      {groups.map((g) => {
        const safePaths = g.items.filter((it) => bucketItem(it) === "safe").map((it) => it.path);
        const suggestPaths = g.items
          .filter((it) => bucketItem(it) === "suggest")
          .map((it) => it.path);
        return (
          <div key={g.origin} style={{ ...css.card, padding: "10px 12px", fontSize: 12.5 }}>
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
              <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
                {safePaths.length > 0 && onToggleMany && (
                  <button
                    type="button"
                    style={{ ...css.btnGhost, height: 26, fontSize: 11 }}
                    onClick={() => onToggleMany(safePaths, true)}
                  >
                    {L.orphanSelectSafe}
                  </button>
                )}
                {suggestPaths.length > 0 && onToggleMany && (
                  <button
                    type="button"
                    style={{ ...css.btnGhost, height: 26, fontSize: 11 }}
                    onClick={() => onToggleMany(suggestPaths, true)}
                  >
                    {L.orphanSelectSuggest}
                  </button>
                )}
              </div>
            </div>
            <div style={{ marginTop: 6, display: "flex", flexDirection: "column", gap: 4 }}>
              {g.items.slice(0, 8).map((it) => {
                const chip = riskChip(it, L);
                const open = openPath === it.path;
                return (
                  <div key={it.path} style={{ borderBottom: "1px solid var(--border)", paddingBottom: 4 }}>
                    <label
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
                        aria-label={it.path}
                        style={{ marginTop: 2, accentColor: "var(--accent)" }}
                      />
                      <span style={{ minWidth: 0, flex: 1 }}>
                        <span
                          className="ell"
                          style={{
                            display: "block",
                            fontFamily: "var(--mono)",
                            fontSize: 11.5,
                            color: "var(--fg)",
                          }}
                          title={it.path}
                        >
                          {it.path}
                        </span>
                        <span style={{ fontSize: 11.5 }}>
                          <span style={{ color: chip.color, fontWeight: 650, marginRight: 8 }}>
                            {chip.label}
                          </span>
                          {(it.reason || "").toLowerCase().includes("orphan") ||
                          (it.reason || "").toLowerCase().includes("leftover")
                            ? L.orphanReasonDefault
                            : it.reason || L.orphanNoOwnerHint}
                        </span>
                      </span>
                      <button
                        type="button"
                        style={{ ...css.btnGhost, height: 24, fontSize: 11, flexShrink: 0 }}
                        onClick={(e) => {
                          e.preventDefault();
                          setOpenPath(open ? null : it.path);
                        }}
                      >
                        {open ? L.orphanCollapseEvidence : L.orphanExpandEvidence}
                      </button>
                    </label>
                    {open && (
                      <div
                        style={{
                          marginTop: 6,
                          marginLeft: 22,
                          padding: "8px 10px",
                          background: "var(--surface-2)",
                          borderRadius: 6,
                          fontSize: 12,
                          color: "var(--fg)",
                        }}
                      >
                        <div style={{ fontWeight: 650, marginBottom: 4 }}>
                          {L.orphanEvidenceTitle}
                        </div>
                        {it.evidence && it.evidence.length > 0 ? (
                          <ul style={{ margin: 0, paddingLeft: 16, color: "var(--muted)" }}>
                            {it.evidence.map((ev) => (
                              <li key={ev.code || ev.detail || ev.label} style={{ marginBottom: 2 }}>
                                <strong style={{ color: "var(--fg)" }}>{ev.label}</strong>
                                {ev.detail ? ` — ${ev.detail}` : ""}
                              </li>
                            ))}
                          </ul>
                        ) : (
                          <div style={{ color: "var(--muted)" }}>{L.orphanNoEvidence}</div>
                        )}
                        <div style={{ marginTop: 6, color: "var(--muted)" }}>
                          {L.orphanRiskLabel}:{" "}
                          {it.risk === "low"
                            ? L.orphanRiskLow
                            : it.risk === "high"
                              ? L.orphanRiskHigh
                              : L.orphanRiskMedium}
                          {it.kind ? ` · ${it.kind}` : ""}
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
              {g.items.length > 8 && <div style={css.muted}>… +{g.items.length - 8}</div>}
            </div>
          </div>
        );
      })}
    </div>
  );
}
