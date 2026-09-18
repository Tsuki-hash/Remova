import { useMemo } from "react";
import { t } from "../i18n";
import { formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { defaultSelectable, summarizeLeftovers } from "../lib/decision";
import type { CleanupItem, ScanResult } from "../types";

type Props = {
  scan: ScanResult;
  scanning?: boolean;
  aiEnabled: boolean;
  aiBusy: boolean;
  aiNote?: string | null;
  onCleanSafe: () => void;
  onShowConfirm: () => void;
  onShowKeep: () => void;
  onExplain: () => void;
  onOpenSettings?: () => void;
};

/** Auto decision layer on top of leftover list (rule-first, AI optional). */
export function CleanupConclusion({
  scan,
  scanning,
  aiEnabled,
  aiBusy,
  aiNote,
  onCleanSafe,
  onShowConfirm,
  onShowKeep,
  onExplain,
  onOpenSettings,
}: Props) {
  const L = t();

  const stats = useMemo(() => {
    const s = summarizeLeftovers(scan.items);
    let safeKb = 0;
    let hasSize = false;
    const keepSamples: CleanupItem[] = [];
    const highRisk = scan.items.filter((i) => i.risk === "high").length;
    for (const it of scan.items) {
      if (defaultSelectable(it)) {
        const kb = it.size_kb;
        if (typeof kb === "number" && kb > 0) {
          safeKb += kb;
          hasSize = true;
        }
      } else if (it.risk === "high" || it.shared || it.user_data) {
        if (keepSamples.length < 3) keepSamples.push(it);
      }
    }
    return { s, safeKb, hasSize, keepSamples, highRisk };
  }, [scan.items]);

  if (scanning) return null;

  const { s, safeKb, hasSize, keepSamples, highRisk } = stats;

  const spaceLine = hasSize ? L.conclusionSpace(formatSize(safeKb)) : L.conclusionSpaceUnknown;
  const riskLine =
    highRisk > 0
      ? L.conclusionMaxRisk(highRisk)
      : s.keep > 0
        ? L.conclusionKeepN(s.keep)
        : L.conclusionNoHighRisk;

  const oneLiner = L.conclusionOneLiner(spaceLine, s.safe, s.keep, riskLine);

  const sourceLabel = aiNote ? L.conclusionSourceAi : L.conclusionSourceRule;

  return (
    <div
      style={{
        ...css.card,
        marginBottom: 10,
        padding: "12px 14px",
        borderLeft: "3px solid var(--accent)",
        background: "var(--surface)",
        flexShrink: 0,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          marginBottom: 6,
          flexWrap: "wrap",
        }}
      >
        <strong style={{ fontSize: 13 }}>{L.conclusionTitle}</strong>
        <span
          style={{
            fontSize: 10.5,
            fontWeight: 650,
            color: aiNote ? "var(--accent)" : "var(--muted)",
            background: aiNote ? "var(--accent-soft)" : "var(--surface-2)",
            border: "1px solid var(--border)",
            borderRadius: 999,
            padding: "1px 8px",
          }}
        >
          {sourceLabel}
        </span>
        {s.total === 0 && <span style={css.muted}>{L.leftoversNone}</span>}
      </div>

      {s.total > 0 && (
        <>
          <div style={{ fontSize: 13, lineHeight: 1.55, marginBottom: 10 }}>{oneLiner}</div>

          {keepSamples.length > 0 && (
            <div style={{ marginBottom: 10, fontSize: 12, color: "var(--muted)", lineHeight: 1.5 }}>
              {keepSamples.map((it) => (
                <div key={it.path} style={{ marginBottom: 2 }}>
                  ·{" "}
                  {it.shared
                    ? L.conclusionSharedHint
                    : it.user_data
                      ? L.conclusionUserDataHint
                      : L.conclusionHighRiskHint}{" "}
                  <span className="ell" style={{ maxWidth: 280, display: "inline-block", verticalAlign: "bottom" }} title={it.path}>
                    {it.path}
                  </span>
                </div>
              ))}
            </div>
          )}

          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
            <button
              style={{ ...css.btn, height: 32 }}
              disabled={s.safe === 0}
              onClick={onCleanSafe}
            >
              {L.conclusionCleanSafe(s.safe)}
            </button>
            <button style={{ ...css.btnGhost, height: 32 }} onClick={onShowConfirm}>
              {L.conclusionShowConfirm(s.suggest)}
            </button>
            <button style={{ ...css.btnGhost, height: 32 }} onClick={onShowKeep}>
              {L.conclusionShowKeep(s.keep)}
            </button>
            {aiEnabled ? (
              <button
                style={{ ...css.btnSm, marginLeft: "auto" }}
                disabled={aiBusy || scan.items.length === 0}
                onClick={onExplain}
              >
                {aiBusy ? L.aiExplaining : L.conclusionRefreshAi}
              </button>
            ) : (
              onOpenSettings && (
                <button
                  style={{ ...css.btnSm, marginLeft: "auto" }}
                  onClick={onOpenSettings}
                  title={L.aiSettingsHint}
                >
                  {L.aiDisabledChip} · {L.conclusionEnableAi}
                </button>
              )
            )}
          </div>

          {aiNote && (
            <div
              style={{
                marginTop: 10,
                paddingTop: 8,
                borderTop: "1px solid var(--border)",
                fontSize: 12,
                lineHeight: 1.5,
                color: "var(--muted)",
              }}
            >
              {aiNote}
              <span style={{ opacity: 0.75 }}> · {L.aiDisclaimer}</span>
            </div>
          )}
          {!aiEnabled && onOpenSettings && (
            <div
              style={{
                marginTop: 10,
                paddingTop: 8,
                borderTop: "1px solid var(--border)",
                fontSize: 12,
                color: "var(--muted)",
                display: "flex",
                gap: 8,
                alignItems: "center",
                flexWrap: "wrap",
              }}
            >
              <span>{L.firstScanAiHint}</span>
              <button
                type="button"
                style={{
                  border: "none",
                  background: "transparent",
                  color: "var(--accent)",
                  fontWeight: 650,
                  cursor: "pointer",
                  padding: 0,
                  fontSize: 12,
                }}
                onClick={onOpenSettings}
              >
                {L.conclusionEnableAi}
              </button>
              <span style={{ opacity: 0.75 }}>· {L.aiNeverDelete}</span>
            </div>
          )}
        </>
      )}
    </div>
  );
}
