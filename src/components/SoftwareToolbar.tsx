import { useEffect, useRef, useState } from "react";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import type { CategoryId } from "../lib/categories";

export type { CategoryId };

/** Beginner toolbar: search + 3 plain filters (store/desktop live in meta only). */
export function SoftwareToolbar({
  q,
  category,
  estimating,
  scanning,
  aiEnabled,
  onQuery,
  onCategory,
  onStopEstimate,
  onOpenAi,
}: {
  q: string;
  category: CategoryId;
  estimating: boolean;
  scanning: boolean;
  aiEnabled: boolean;
  onQuery: (v: string) => void;
  onCategory: (id: CategoryId) => void;
  onStopEstimate: () => void;
  onOpenAi: () => void;
}) {
  const L = t();
  const [guideOpen, setGuideOpen] = useState(false);
  const guideRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!guideOpen) return;
    const onDown = (e: MouseEvent) => {
      if (guideRef.current && !guideRef.current.contains(e.target as Node)) {
        setGuideOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setGuideOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [guideOpen]);

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
      <button
        type="button"
        onClick={onOpenAi}
        title={aiEnabled ? L.aiEnabledChip : L.aiDisabledChip}
        style={{
          height: 32,
          padding: "0 12px",
          borderRadius: 8,
          border: `1px solid ${aiEnabled ? "var(--accent)" : "var(--border)"}`,
          background: aiEnabled ? "var(--accent-soft)" : "var(--surface)",
          color: aiEnabled ? "var(--accent)" : "var(--muted)",
          fontSize: 12,
          fontWeight: 650,
          cursor: "pointer",
          flexShrink: 0,
          whiteSpace: "nowrap",
        }}
      >
        {aiEnabled ? L.aiEnabledChip : L.aiDisabledChip}
      </button>
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
      <div ref={guideRef} style={{ position: "relative", flexShrink: 0 }}>
        <button
          type="button"
          aria-label={L.guided}
          aria-expanded={guideOpen}
          title={L.guided}
          onClick={() => setGuideOpen((v) => !v)}
          style={{
            width: 22,
            height: 22,
            borderRadius: 999,
            border: "1px solid var(--border)",
            background: guideOpen ? "var(--accent-soft)" : "transparent",
            color: guideOpen ? "var(--accent)" : "var(--muted)",
            display: "grid",
            placeItems: "center",
            cursor: "pointer",
            fontSize: 12,
            lineHeight: 1,
            padding: 0,
          }}
        >
          ⓘ
        </button>
        {guideOpen && (
          <div
            role="dialog"
            aria-label={L.guided}
            style={{
              position: "absolute",
              top: 28,
              right: 0,
              zIndex: 40,
              width: 280,
              padding: "10px 12px",
              borderRadius: 10,
              border: "1px solid var(--border)",
              background: "var(--surface)",
              boxShadow: "0 8px 24px rgba(0,0,0,.12)",
              fontSize: 12.5,
              lineHeight: 1.55,
              color: "var(--fg)",
            }}
          >
            <div style={{ marginBottom: 8 }}>{L.guided}</div>
            <button
              type="button"
              style={{ ...css.btnSm, height: 26, width: "100%" }}
              onClick={() => setGuideOpen(false)}
            >
              {L.closeGuide}
            </button>
          </div>
        )}
      </div>
      {estimating && (
        <button style={{ ...css.btnSm, height: 32 }} title={L.stopEstimateHint} onClick={onStopEstimate}>
          {L.stopEstimate}
        </button>
      )}
      {scanning && <span style={{ ...css.muted, flexShrink: 0 }}>{L.analyzing}</span>}
    </div>
  );
}
