import { useEffect, useState, type ReactNode } from "react";
import type { NavId } from "../lib/theme";
import { t } from "../i18n";
import { WindowControls } from "./WindowControls";

const NAV_ICONS: Record<NavId, string> = {
  software: "▣",
  startup: "⚡",
  services: "⚙",
  tasks: "⏱",
  orphans: "◎",
  more: "•••",
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
  const items: { id: NavId; label: string }[] = [
    { id: "software", label: L.navSoftware },
    { id: "startup", label: L.navStartup },
    { id: "services", label: L.navServices },
    { id: "tasks", label: L.navTasks },
    { id: "orphans", label: L.navOrphans },
    { id: "more", label: L.navMore },
  ];

  return (
    <nav
      aria-label="Main"
      style={{
        width: collapsed ? 64 : 200,
        flexShrink: 0,
        background: "var(--surface)",
        borderRight: "1px solid var(--border)",
        display: "flex",
        flexDirection: "column",
        padding: "14px 10px",
        gap: 4,
        transition: "width .15s ease",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "6px 10px 16px",
          minWidth: 0,
        }}
      >
        <img
          src="/logo.png"
          alt=""
          width={32}
          height={32}
          style={{ display: "block", width: 32, height: 32, flexShrink: 0 }}
        />
        {!collapsed && (
          <div style={{ minWidth: 0 }}>
            <div style={{ fontWeight: 700, fontSize: 15, letterSpacing: -0.2 }}>Remova</div>
            <div style={{ fontSize: 11, color: "var(--muted)" }}>{L.subtitle}</div>
          </div>
        )}
      </div>

      {items.map((it) => {
        const active = nav === it.id;
        return (
          <button
            key={it.id}
            onClick={() => onNav(it.id)}
            title={it.label}
            aria-current={active ? "page" : undefined}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              width: "100%",
              border: "none",
              borderRadius: 8,
              padding: collapsed ? "10px 0" : "10px 12px",
              background: active ? "var(--accent-soft)" : "transparent",
              color: active ? "var(--accent)" : "var(--fg)",
              fontWeight: active ? 650 : 500,
              fontSize: 13.5,
              cursor: "pointer",
              justifyContent: collapsed ? "center" : "flex-start",
              position: "relative",
              textAlign: "left" as const,
            }}
          >
            <span style={{ width: 20, textAlign: "center", opacity: active ? 1 : 0.75 }}>
              {NAV_ICONS[it.id]}
            </span>
            {!collapsed && <span className="ell">{it.label}</span>}
          </button>
        );
      })}
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
  return (
    <div style={{ display: "flex", height: "100vh", overflow: "hidden", background: "var(--bg)" }}>
      <SidebarNav nav={nav} onNav={onNav} />
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
        <header
          data-tauri-drag-region
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "12px 8px 12px 20px",
            borderBottom: "1px solid var(--border)",
            background: "var(--surface)",
            flexShrink: 0,
            userSelect: "none",
          }}
        >
          <div style={{ minWidth: 0, flex: 1 }} data-tauri-drag-region>
            <h1 style={{ margin: 0, fontSize: 20, fontWeight: 700, letterSpacing: -0.3 }} data-tauri-drag-region>
              {title}
            </h1>
            <div
              data-tauri-drag-region
              style={{ display: "flex", gap: 12, flexWrap: "wrap", alignItems: "center", marginTop: 4 }}
            >
              {subtitle && (
                <div style={{ fontSize: 12.5, color: "var(--muted)" }} data-tauri-drag-region>
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
                    fontSize: 12.5,
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
          style={{
            flex: 1,
            minHeight: 0,
            // block + own scroll: flex-column children were being height-shrunk
            // (squashing banners and pushing tool panels off-screen).
            display: "block",
            overflow: "auto",
            padding: "10px 14px 10px",
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
              padding: "6px 14px",
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
