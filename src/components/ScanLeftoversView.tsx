import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { LeftoverSummaryBar } from "./LeftoverSummaryBar";
import { OrphanOriginGroups } from "./OrphanOriginGroups";
import {
  flattenOriginGroups,
  groupByOrigin,
  leftoverReasonLine,
  summarizeLeftovers,
} from "../lib/decision";
import type { ScanResult } from "../types";

type Props = {
  scan: ScanResult;
  scanning: boolean;
  selectedPaths: Set<string>;
  evidence: string | null;
  aiNotes: Record<string, string>;
  orphanLabel: string;
  onTogglePath: (path: string) => void;
  onEvidence: (text: string | null) => void;
};

/** Leftover table + summary + optional orphan origin groups. */
export function ScanLeftoversView({
  scan,
  scanning,
  selectedPaths,
  evidence,
  aiNotes,
  orphanLabel,
  onTogglePath,
  onEvidence,
}: Props) {
  const L = t();
  const isOrphan = scan.app_name === orphanLabel;
  const originGroups = isOrphan ? groupByOrigin(scan.items) : [];
  const displayItems = isOrphan ? flattenOriginGroups(originGroups) : scan.items;
  return (
    <div style={{ ...css.card, flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
      <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
        <strong>{scan.app_name}</strong>
        <span style={{ ...css.muted, marginLeft: 12 }}>
          {L.leftoversTitle}: {scan.items.length} ·{" "}
          {scan.items.filter((i) => i.confidence === "confirmed").length} {L.confirmed}
        </span>
      </div>
      <LeftoverSummaryBar summary={summarizeLeftovers(scan.items)} scanning={scanning} />
      <div style={css.scroll}>
        {isOrphan && originGroups.length > 0 && (
          <div style={{ padding: "10px 14px 0" }}>
            <OrphanOriginGroups
              groups={originGroups}
              selectedPaths={selectedPaths}
              onToggle={onTogglePath}
            />
          </div>
        )}
        <table style={css.table}>
          <thead>
            <tr>
              <th style={css.th}>✓</th>
              <th style={css.th}>{L.colLocation}</th>
              <th style={css.th}>{L.colSource}</th>
              <th style={css.th}>{L.confirmed}</th>
              <th style={css.th}>ⓘ</th>
            </tr>
          </thead>
          <tbody>
            {displayItems.length === 0 && (
              <tr>
                <td colSpan={5} style={{ ...css.td, color: "var(--muted)" }}>
                  {L.leftoversNone}
                </td>
              </tr>
            )}
            {displayItems.map((it) => (
              <tr
                key={it.path}
                style={{
                  boxShadow: it.risk === "high" ? "inset 3px 0 0 var(--danger)" : undefined,
                }}
              >
                <td style={css.td}>
                  <input
                    type="checkbox"
                    checked={selectedPaths.has(it.path)}
                    onChange={() => onTogglePath(it.path)}
                  />
                </td>
                <td style={css.td}>
                  <span className="ell" style={{ display: "block" }} title={it.path}>
                    {it.path}
                  </span>
                  <div style={{ fontSize: 11.5, color: "var(--muted)", marginTop: 2, lineHeight: 1.4 }}>
                    {leftoverReasonLine(it, L)}
                    {it.reason && it.reason !== leftoverReasonLine(it, L) ? ` · ${it.reason}` : ""}
                  </div>
                  {aiNotes[it.path] && (
                    <div style={{ fontSize: 11.5, color: "var(--muted)", marginTop: 2, lineHeight: 1.4 }}>
                      ✦ {aiNotes[it.path]}
                      <span style={{ opacity: 0.75 }}> · {L.aiDisclaimer}</span>
                    </div>
                  )}
                </td>
                <td style={css.td}>
                  <span style={css.sourceBadge}>{it.kind}</span>
                  {it.shared && (
                    <span
                      style={{
                        ...css.sourceBadge,
                        marginLeft: 6,
                        color: "var(--warn)",
                        borderColor: "var(--warn)",
                      }}
                      title={L.sharedHint}
                    >
                      {L.badgeShared}
                    </span>
                  )}
                  {it.user_data && (
                    <span
                      style={{
                        ...css.sourceBadge,
                        marginLeft: 6,
                        color: "var(--danger)",
                        borderColor: "var(--danger)",
                      }}
                      title={L.userDataHint}
                    >
                      {L.badgeUserData}
                    </span>
                  )}
                </td>
                <td style={css.td}>
                  <span
                    style={{
                      color:
                        it.risk === "high"
                          ? "var(--danger)"
                          : it.confidence === "confirmed"
                            ? "var(--ok)"
                            : "var(--warn)",
                      fontWeight: 600,
                      fontSize: 12,
                    }}
                  >
                    {it.risk === "high"
                      ? L.riskHigh
                      : it.confidence === "confirmed"
                        ? L.confirmed
                        : it.score >= 30
                          ? L.suspected
                          : L.low}
                  </span>
                </td>
                <td style={css.td}>
                  <button
                    style={{ ...css.btnGhost, height: 28 }}
                    onClick={() =>
                      onEvidence(
                        it.evidence
                          .map((e) => `${e.label} (${e.weight})${e.detail ? " — " + e.detail : ""}`)
                          .join("\n") || it.reason,
                      )
                    }
                  >
                    ⓘ
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {evidence && (
        <div
          style={{
            padding: 12,
            borderTop: "1px solid var(--border)",
            fontSize: 13,
            whiteSpace: "pre-wrap",
            background: "var(--th-bg)",
            flexShrink: 0,
          }}
        >
          {evidence}{" "}
          <button style={{ ...css.btnGhost, height: 28 }} onClick={() => onEvidence(null)}>
            ×
          </button>
        </div>
      )}
    </div>
  );
}
