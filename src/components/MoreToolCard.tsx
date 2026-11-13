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
  | "toolcache"
  | "broken"
  | "firewall"
  | "onboarding"
  | "diskoverview"
  | "cliglobals"
  | "migrate"
  | "traces"
  | "vectors"
  | "msi"
  | "toolaudit";

export type ToolItem = {
  id: ToolId;
  title: string;
  desc: string;
  icon: ToolIconName;
  action: () => void;
  badge?: string;
  disabled?: boolean;
};

export function ToolCard({
  item,
  active,
  quick = false,
  primary = false,
}: {
  item: ToolItem;
  active: boolean;
  quick?: boolean;
  primary?: boolean;
}) {
  return (
    <button
      disabled={item.disabled}
      className="remova-tool-card"
      data-quick={quick || undefined}
      data-primary={primary || undefined}
      type="button"
      title={quick ? undefined : item.desc}
      aria-label={item.title}
      aria-description={item.badge ? `${item.desc} · ${item.badge}` : item.desc}
      aria-expanded={active}
      aria-controls={active ? "remova-more-tool-panel" : undefined}
      onClick={item.action}
    >
      <span className="remova-tool-icon" aria-hidden>
        <ToolGlyph name={item.icon} size={quick ? 33 : 23} />
      </span>
      <span className="remova-tool-label">
        <span className="remova-tool-name">
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
        </span>
        {quick && <span className="remova-tool-description">{item.desc}</span>}
      </span>
      <span style={{ color: "var(--muted)", fontSize: 14, alignSelf: "center" }} aria-hidden>
        ›
      </span>
    </button>
  );
}
