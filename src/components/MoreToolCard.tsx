import type { CSSProperties } from "react";
import { ToolGlyph, type ToolIconName } from "./ToolIcons";

export type ToolId =
  | "history"
  | "restore"
  | "force"
  | "orphan"
  | "monitor"
  | "ignore"
  | "shell"
  | "releases"
  | "open-releases"
  | "csv"
  | "report"
  | "ai"
  | "idle"
  | "installers"
  | "diskradar"
  | "toolcache";

export type ToolItem = {
  id: ToolId;
  title: string;
  desc: string;
  icon: ToolIconName;
  action: () => void;
  accent?: boolean;
  badge?: string;
};

export const cardBase: CSSProperties = {
  width: "100%",
  textAlign: "left",
  border: "1px solid var(--border)",
  background: "var(--surface)",
  borderRadius: 12,
  padding: "18px 16px",
  cursor: "pointer",
  display: "flex",
  gap: 12,
  alignItems: "flex-start",
  transition: "border-color .12s, box-shadow .12s, background .12s",
};

export function ToolCard({
  item,
  active,
}: {
  item: ToolItem;
  active: boolean;
}) {
  return (
    <button
      className="remova-tool-card"
      type="button"
      onClick={item.action}
      style={{
        ...cardBase,
        borderColor: active ? "var(--accent)" : "var(--border)",
        background: active ? "var(--accent-soft)" : "var(--surface)",
        boxShadow: "none",
      }}
          onMouseEnter={(e) => {
        if (active) return;
        e.currentTarget.style.borderColor = "var(--border-strong)";
        e.currentTarget.style.boxShadow = "0 1px 0 rgba(0,0,0,.04)";
      }}
      onMouseLeave={(e) => {
        if (active) return;
        e.currentTarget.style.borderColor = "var(--border)";
        e.currentTarget.style.boxShadow = "none";
      }}
      onFocus={(e) => {
 // keyboard focus must be as visible as hover.
        if (active) return;
        e.currentTarget.style.borderColor = "var(--border-strong)";
        e.currentTarget.style.boxShadow = "0 0 0 2px var(--accent-soft)";
      }}
      onBlur={(e) => {
        if (active) return;
        e.currentTarget.style.borderColor = "var(--border)";
        e.currentTarget.style.boxShadow = "none";
      }}
    >
      <span
        style={{
          width: 34,
          height: 34,
          borderRadius: 10,
          background: item.accent ? "var(--accent-soft)" : "var(--surface-2)",
          color: item.accent ? "var(--accent)" : "var(--muted)",
          display: "grid",
          placeItems: "center",
          fontSize: 16,
          fontWeight: 600,
          flexShrink: 0,
        }}
        aria-hidden
      >
        <ToolGlyph name={item.icon} />
      </span>
      <span style={{ minWidth: 0, flex: 1 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
          <span style={{ fontWeight: 600, fontSize: 13 }}>{item.title}</span>
          {item.badge && (
            <span
              style={{
                fontSize: 10.5,
                fontWeight: 650,
                letterSpacing: 0.2,
                color: "var(--accent-text)",
                background: "var(--accent-soft)",
                borderRadius: 999,
                padding: "1px 7px",
              }}
            >
              {item.badge}
            </span>
          )}
        </div>
        <div style={{ color: "var(--muted)", fontSize: 12, lineHeight: 1.45 }}>{item.desc}</div>
      </span>
      <span style={{ color: "var(--muted)", fontSize: 14, alignSelf: "center" }} aria-hidden>
        ›
      </span>
    </button>
  );
}
