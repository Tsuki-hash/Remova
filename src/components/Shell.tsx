import { useEffect, useState, type ReactNode } from "react";
import type { NavId } from "../lib/theme";
import { t } from "../i18n";

const NAV_ICONS: Record<NavId, string> = {
  software: "▣",
  startup: "⚡",
  services: "⚙",
  tasks: "⏱",
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
        <div
          style={{
            width: 32,
            height: 32,
            borderRadius: 10,
            background: "linear-gradient(145deg, var(--accent), #6C5CE7)",
            color: "#fff",
            display: "grid",
            placeItems: "center",
            fontWeight: 700,
            fontSize: 14,
            flexShrink: 0,
          }}
        >
          R
        </div>
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
              borderRadius: 10,
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
            {!collapsed && active && (
              <span
                style={{
                  position: "absolute",
                  left: 0,
                  top: 8,
                  bottom: 8,
                  width: 3,
                  borderRadius: 2,
                  background: "var(--accent)",
                }}
              />
            )}
            <span style={{ width: 20, textAlign: "center", opacity: 0.9 }}>{NAV_ICONS[it.id]}</span>
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
  children,
}: {
  nav: NavId;
  onNav: (n: NavId) => void;
  title: string;
  subtitle?: string;
  actions?: ReactNode;
  status?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div style={{ display: "flex", height: "100vh", overflow: "hidden", background: "var(--bg)" }}>
      <SidebarNav nav={nav} onNav={onNav} />
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
        <header
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "14px 18px 12px",
            borderBottom: "1px solid var(--border)",
            background: "var(--surface)",
            flexShrink: 0,
          }}
        >
          <div style={{ minWidth: 0 }}>
            <h1 style={{ margin: 0, fontSize: 18, fontWeight: 700, letterSpacing: -0.2 }}>{title}</h1>
            {subtitle && (
              <div style={{ fontSize: 12, color: "var(--muted)", marginTop: 2 }}>{subtitle}</div>
            )}
          </div>
          {status && (
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap", alignItems: "center" }}>
              {status}
            </div>
          )}
          <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
            {actions}
          </div>
        </header>
        <main
          style={{
            flex: 1,
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
            padding: "12px 16px 14px",
            overflow: "hidden",
          }}
        >
          {children}
        </main>
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
