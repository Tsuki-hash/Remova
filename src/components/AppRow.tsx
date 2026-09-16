import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { InstalledApp } from "../types";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { AppIcon } from "./AppIcon";
import { prettyAppName } from "../lib/format";
import { decisionChips, type DecisionChip } from "../lib/decision";

function chipStyle(c: DecisionChip) {
  if (c.tone === "accent") return { ...css.chipAccent, fontFamily: "inherit" as const };
  if (c.tone === "danger") return { ...css.chipDanger, fontFamily: "inherit" as const };
  if (c.tone === "warn") {
    return {
      ...css.chip,
      fontFamily: "inherit" as const,
      color: "var(--warn)",
      borderColor: "var(--warn)",
    };
  }
  if (c.tone === "ok") {
    return { ...css.chip, fontFamily: "inherit" as const, color: "var(--ok)" };
  }
  return { ...css.chip, fontFamily: "inherit" as const };
}

type Props = {
  app: InstalledApp;
  rowKey: string;
  selected: boolean;
  checked: boolean;
  sizeText: string;
  sizeKb: number;
  uninstalling: boolean;
  onSelect: (app: InstalledApp) => void;
  onUninstall: (app: InstalledApp) => void;
  onAnalyze: (app: InstalledApp) => void;
  onForceClean: (app: InstalledApp) => void;
  onIgnoreApp: (app: InstalledApp) => void;
  onIgnorePub: (app: InstalledApp) => void;
  onToggleMulti: (key: string) => void;
  onEnsureSelected: (app: InstalledApp) => void;
};

function RowMenu({
  app,
  onAnalyze,
  onForceClean,
  onIgnoreApp,
  onIgnorePub,
}: {
  app: InstalledApp;
  onAnalyze: (a: InstalledApp) => void;
  onForceClean: (a: InstalledApp) => void;
  onIgnoreApp: (a: InstalledApp) => void;
  onIgnorePub: (a: InstalledApp) => void;
}) {
  const L = t();
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState<{ top: number; left: number; minWidth: number } | null>(null);

  const items = [
    { id: "analyze", label: L.rowAnalyze, fn: onAnalyze },
    { id: "force", label: L.rowForceClean, fn: onForceClean },
    { id: "ignore-app", label: L.rowIgnoreApp, fn: onIgnoreApp },
    ...(app.publisher
      ? [{ id: "ignore-pub", label: L.rowIgnorePub, fn: onIgnorePub }]
      : []),
  ];

  // Fixed + portal so virtualized list overflow does not clip the menu.
  useLayoutEffect(() => {
    if (!open || !btnRef.current) return;
    const r = btnRef.current.getBoundingClientRect();
    const menuH = 40 + items.length * 34;
    const spaceBelow = window.innerHeight - r.bottom;
    const flip = spaceBelow < menuH + 8 && r.top > menuH + 8;
    const top = flip ? r.top - menuH - 4 : r.bottom + 4;
    const minWidth = 168;
    const left = Math.max(8, Math.min(r.right - minWidth, window.innerWidth - minWidth - 8));
    setPos({ top, left, minWidth });
  }, [open, items.length]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (
        menuRef.current?.contains(e.target as Node) ||
        btnRef.current?.contains(e.target as Node)
      ) {
        return;
      }
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    const onScroll = () => setOpen(false);
    document.addEventListener("mousedown", onDoc);
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", onScroll);
    // Scroll closes menu — re-open on the new row position.
    document.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onScroll);
      document.removeEventListener("scroll", onScroll, true);
    };
  }, [open]);

  return (
    <div style={{ position: "relative", display: "inline-block" }}>
      <button
        ref={btnRef}
        style={{
          ...css.btnSm,
          width: 32,
          height: 30,
          padding: 0,
          color: "var(--muted)",
        }}
        title={L.rowMore}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
      >
        ⋯
      </button>
      {open &&
        pos &&
        createPortal(
          <div
            ref={menuRef}
            role="menu"
            style={{
              position: "fixed",
              top: pos.top,
              left: pos.left,
              zIndex: 10000,
              minWidth: pos.minWidth,
              background: "var(--surface)",
              border: "1px solid var(--border)",
              borderRadius: 10,
              boxShadow: "var(--shadow)",
              padding: 4,
            }}
          >
            {items.map((it) => (
              <button
                key={it.id}
                role="menuitem"
                style={{
                  display: "block",
                  width: "100%",
                  textAlign: "left" as const,
                  border: "none",
                  background: "transparent",
                  color: "var(--fg)",
                  padding: "8px 10px",
                  borderRadius: 6,
                  fontSize: 12.5,
                  cursor: "pointer",
                  whiteSpace: "nowrap" as const,
                }}
                onClick={(e) => {
                  e.stopPropagation();
                  setOpen(false);
                  it.fn(app);
                }}
              >
                {it.label}
              </button>
            ))}
          </div>,
          document.body,
        )}
    </div>
  );
}

function AppRowImpl({
  app,
  rowKey,
  selected,
  checked,
  sizeText,
  sizeKb,
  uninstalling,
  onSelect,
  onUninstall,
  onAnalyze,
  onForceClean,
  onIgnoreApp,
  onIgnorePub,
  onToggleMulti,
  onEnsureSelected,
}: Props) {
  const L = t();
  const a = app;
  const hasUninstall = Boolean(
    (a.quiet_uninstall_string || a.uninstall_string || "").trim(),
  );
  const chips = decisionChips(a, sizeKb, L, { compact: true });
  return (
    <tr
      onClick={() => onSelect(a)}
      title={[
        prettyAppName(a.name, a.source),
        a.version && `${L.detailVersion}: ${a.version}`,
        a.publisher && `${L.detailPublisher}: ${a.publisher}`,
        a.install_date && `${L.detailDate}: ${a.install_date}`,
        a.install_location && `${L.detailPath}: ${a.install_location}`,
        a.registry_key && `${L.colRegKey}: ${a.registry_key}`,
      ]
        .filter(Boolean)
        .join("\n") || undefined}
      style={{
        cursor: "pointer",
        background: selected ? "var(--row-selected)" : undefined,
        boxShadow: selected ? "inset 2px 0 0 var(--accent)" : undefined,
      }}
    >
      <td style={{ ...css.td, width: 40, textAlign: "center" as const, paddingTop: 12, paddingBottom: 12 }}>
        <input
          type="checkbox"
          checked={checked}
          onChange={() => {
            const next = !checked;
            onToggleMulti(rowKey);
            if (next && !selected) onEnsureSelected(a);
          }}
          onClick={(e) => e.stopPropagation()}
          style={{ accentColor: "var(--accent)", cursor: "pointer", margin: 0 }}
          aria-label={prettyAppName(a.name, a.source)}
        />
      </td>
      <td style={{ ...css.td, paddingTop: 6, paddingBottom: 6 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
          <AppIcon displayIcon={a.display_icon} name={a.name} />
          <div style={{ minWidth: 0 }}>
            <div
              className="ell"
              style={{ fontWeight: selected ? 650 : 550, minWidth: 0, fontSize: 13 }}
              title={a.name}
            >
              {prettyAppName(a.name, a.source)}
            </div>
            <div
              className="ell"
              style={{ fontSize: 11, color: "var(--muted)", maxWidth: 360 }}
              title={a.publisher}
            >
              {[
                a.source === "Store" ? L.chipStore : L.chipDesktop,
                a.publisher || "",
                a.install_date || "",
              ]
                .filter(Boolean)
                .join(" · ")}
            </div>
            {chips.length > 0 && (
              <div style={{ display: "flex", gap: 4, flexWrap: "wrap", marginTop: 2 }}>
                {chips.map((c) => (
                  <span
                    key={c.id}
                    title={c.title}
                    style={{ ...chipStyle(c), height: 16, fontSize: 10, padding: "0 5px" }}
                  >
                    {c.label}
                  </span>
                ))}
              </div>
            )}
          </div>
        </div>
      </td>
      <td
        style={{
          ...css.td,
          ...css.mono,
          textAlign: "right" as const,
          whiteSpace: "nowrap",
          width: 100,
          paddingTop: 6,
          paddingBottom: 6,
          color: sizeText === "—" ? "var(--muted)" : "var(--fg)",
        }}
      >
        {sizeText}
      </td>
      <td style={{ ...css.td, width: 120, textAlign: "right" as const, paddingTop: 4, paddingBottom: 4 }}>
        <div style={{ display: "inline-flex", gap: 4, alignItems: "center" }}>
          <button
            className="remova-row-uninstall"
            style={{
              ...css.btnSm,
              height: 28,
              color: "var(--accent)",
              borderColor: "var(--border)",
              background: "var(--surface)",
              fontWeight: 550,
              opacity: uninstalling ? 0.6 : 1,
            }}
            disabled={!hasUninstall || uninstalling}
            title={hasUninstall ? L.uninstallConfirm(prettyAppName(a.name, a.source)) : L.uninstallNoCmd}
            onClick={(e) => {
              e.stopPropagation();
              onUninstall(a);
            }}
          >
            {uninstalling ? L.uninstalling : L.uninstall}
          </button>
          <RowMenu
            app={a}
            onAnalyze={(x) => {
              onAnalyze(x);
            }}
            onForceClean={(x) => {
              onForceClean(x);
            }}
            onIgnoreApp={(x) => {
              onIgnoreApp(x);
            }}
            onIgnorePub={(x) => {
              onIgnorePub(x);
            }}
          />
        </div>
      </td>
    </tr>
  );
}

export const AppRow = memo(AppRowImpl);
