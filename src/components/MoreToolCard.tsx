import type { CSSProperties } from "react";
import { t } from "../i18n";
import type { InstalledApp } from "../types";

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
  icon: string;
  action: () => void;
  needsSelection?: boolean;
  accent?: boolean;
  badge?: string;
};

export const cardBase: CSSProperties = {
  width: "100%",
  textAlign: "left",
  border: "1px solid var(--border)",
  background: "var(--surface)",
  borderRadius: 10,
  padding: "14px 14px 12px",
  cursor: "pointer",
  display: "flex",
  gap: 12,
  alignItems: "flex-start",
  transition: "border-color .12s, box-shadow .12s, background .12s",
};

export function ToolCard({
  item,
  active,
  selectedApp,
}: {
  item: ToolItem;
  active: boolean;
  selectedApp: InstalledApp | null;
}) {
  const L = t();
  const blocked = item.needsSelection && !selectedApp;
  return (
    <button
      type="button"
      onClick={item.action}
      style={{
        ...cardBase,
        borderColor: active ? "var(--accent)" : "var(--border)",
        background: active ? "var(--accent-soft)" : "var(--surface)",
        boxShadow: active ? "0 0 0 1px var(--accent)" : "none",
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
    >
      <span
        style={{
          width: 36,
          height: 36,
          borderRadius: 9,
          background: item.accent && !blocked ? "var(--accent-soft)" : "var(--surface-2)",
          color: item.accent && !blocked ? "var(--accent)" : "var(--muted)",
          display: "grid",
          placeItems: "center",
          fontSize: 16,
          fontWeight: 600,
          flexShrink: 0,
          border:
            item.accent && !blocked ? "1px solid transparent" : "1px solid var(--border)",
        }}
        aria-hidden
      >
        {item.icon}
      </span>
      <span style={{ minWidth: 0, flex: 1 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
          <span style={{ fontWeight: 650, fontSize: 13.5 }}>{item.title}</span>
          {item.badge && (
            <span
              style={{
                fontSize: 10.5,
                fontWeight: 650,
                letterSpacing: 0.2,
                color: "var(--accent)",
                background: "var(--accent-soft)",
                borderRadius: 999,
                padding: "1px 7px",
              }}
            >
              {item.badge}
            </span>
          )}
        </div>
        <div style={{ color: "var(--muted)", fontSize: 12, lineHeight: 1.45 }}>
          {blocked ? L.needsSoftware : item.desc}
        </div>
        {blocked && (
          <div
            style={{
              marginTop: 8,
              fontSize: 12,
              fontWeight: 600,
              color: "var(--accent)",
            }}
          >
            {L.goToSoftware} →
          </div>
        )}
      </span>
      <span style={{ color: "var(--muted)", fontSize: 14, alignSelf: "center" }} aria-hidden>
        ›
      </span>
    </button>
  );
}
