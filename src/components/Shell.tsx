import { useEffect, useState, type ReactNode } from "react";
import type { NavId } from "../lib/theme";
import { t } from "../i18n";
import { WindowControls } from "./WindowControls";
import { ToolGlyph, type ToolIconName } from "./ToolIcons";

const NAV_ICONS: Record<NavId, ToolIconName> = {
  software: "apps",
  startup: "startup",
  services: "services",
  tasks: "tasks",
  orphans: "cleanup",
  more: "toolbox",
  settings: "settings",
};

export function Sidebar({
  nav,
  onNav,
  collapsed,
}: {
  nav: NavId;
  onNav: (n: NavId) => void;
  collapsed: boolean;
}) {
  const L = t();
  const button = (id: NavId, label: string) => (
    <button
      type="button"
      onClick={() => onNav(id)}
      aria-label={label}
      aria-current={nav === id ? "page" : undefined}
    >
      <ToolGlyph name={NAV_ICONS[id]} size={20} filled={nav === id && (id === "software" || id === "startup" || id === "services")} />
      <span className="remova-navigation-label">{label}</span>
    </button>
  );

  return (
    <nav
      className="remova-navigation"
      aria-label={L.navRegion}
      data-collapsed={collapsed || undefined}
    >
      <div className="remova-navigation-brand">
        <img src="/logo.png" alt="" width={32} height={32} />
        {!collapsed && <strong>Remova</strong>}
      </div>
      {button("software", L.navSoftware)}
      <div className="remova-navigation-group" role="group" aria-label={L.navSystemGroup}>
        {!collapsed && <span className="remova-navigation-caption" aria-hidden>{L.navSystemGroup}</span>}
        {button("startup", L.navStartup)}
        {button("services", L.navServices)}
        {button("tasks", L.navTasks)}
        {button("orphans", L.navOrphans)}
      </div>
      <div className="remova-navigation-tools">{button("more", L.navMore)}</div>
      <div className="remova-navigation-bottom">{button("settings", L.moreSectionHelp)}</div>
    </nav>
  );
}

export function Shell({
  nav,
  onNav,
  title,
  subtitle,
  actions,
  status,
  footer,
  children,
}: {
  nav: NavId;
  onNav: (n: NavId) => void;
  title: string;
  subtitle?: string;
  actions?: ReactNode;
  status?: ReactNode;
  footer?: ReactNode;
  children: ReactNode;
}) {
  const maintenance = nav === "more" || nav === "settings" || nav === "tasks";
  return (
    <div
      className="remova-shell"
      data-maintenance={maintenance || undefined}
      style={{ display: "flex", height: "100vh", overflow: "hidden", background: "var(--bg)" }}
    >
      <SidebarNav nav={nav} onNav={onNav} />
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
        <header
          data-tauri-drag-region
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "16px 8px 16px 22px",
            borderBottom: "1px solid var(--border)",
            background: "var(--surface)",
            flexShrink: 0,
            userSelect: "none",
          }}
        >
          <div style={{ minWidth: 0, flex: 1 }} data-tauri-drag-region>
            <h1 style={{ margin: 0, fontSize: maintenance ? 26 : 22, fontWeight: 650, letterSpacing: -0.4 }} data-tauri-drag-region>
              {title}
            </h1>
            <div
              data-tauri-drag-region
              style={{ display: "flex", gap: 12, flexWrap: "wrap", alignItems: "center", marginTop: 4 }}
            >
              {subtitle && (
                <div style={{ fontSize: 13, color: "var(--muted)" }} data-tauri-drag-region>
                  {subtitle}
                </div>
              )}
              {status && (
                <div
                  style={{
                    display: "flex",
                    gap: 12,
                    flexWrap: "wrap",
                    alignItems: "center",
                    fontSize: 13,
                    color: "var(--muted)",
                  }}
                >
                  {status}
                </div>
              )}
            </div>
          </div>
          <div
            style={{
              marginLeft: "auto",
              display: "flex",
              gap: 8,
              alignItems: "center",
 // Keep controls clickable; only the chrome around them is draggable.
            }}
          >
            {actions}
            <WindowControls />
          </div>
        </header>
        <main
          className="remova-workspace"
          style={{
            flex: 1,
            minHeight: 0,
 // block + own scroll: flex-column children were being height-shrunk
 // (squashing banners and pushing tool panels off-screen).
            display: "block",
            overflow: "auto",
            padding: "16px 20px",
          }}
        >
          {children}
        </main>
        {footer && (
          <footer
            style={{
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
              gap: 14,
              flexWrap: "wrap",
              padding: "8px 20px",
              borderTop: "1px solid var(--border)",
              background: "var(--surface)",
              fontSize: 11.5,
              color: "var(--muted)",
              minHeight: 30,
            }}
          >
            {footer}
          </footer>
        )}
      </div>
    </div>
  );
}

/** Responsive wrapper: collapses sidebar on narrow windows. */
function SidebarNav({ nav, onNav }: { nav: NavId; onNav: (n: NavId) => void }) {
  const [narrow, setNarrow] = useState(
    typeof window !== "undefined" ? window.innerWidth < 1100 : false,
  );
  useEffect(() => {
    const onResize = () => setNarrow(window.innerWidth < 1100);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return <Sidebar nav={nav} onNav={onNav} collapsed={narrow} />;
}
