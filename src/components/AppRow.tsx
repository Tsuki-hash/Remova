import { memo, useEffect, useRef, useState } from "react";
import type { InstalledApp } from "../types";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { AppIcon } from "./AppIcon";
import { prettyAppName } from "../lib/format";

type Props = {
  app: InstalledApp;
  rowKey: string;
  selected: boolean;
  checked: boolean;
  sizeText: string;
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
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const items = [
    { id: "analyze", label: L.rowAnalyze, fn: onAnalyze },
    { id: "force", label: L.rowForceClean, fn: onForceClean },
    { id: "ignore-app", label: L.rowIgnoreApp, fn: onIgnoreApp },
    ...(app.publisher
      ? [{ id: "ignore-pub", label: L.rowIgnorePub, fn: onIgnorePub }]
      : []),
  ];

  return (
    <div ref={ref} style={{ position: "relative", display: "inline-block" }}>
      <button
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
      {open && (
        <div
          role="menu"
          style={{
            position: "absolute",
            right: 0,
            top: "calc(100% + 2px)",
            zIndex: 30,
            minWidth: 160,
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
        </div>
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
      <td style={{ ...css.td, paddingTop: 10, paddingBottom: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
          <AppIcon displayIcon={a.display_icon} name={a.name} />
          <div style={{ minWidth: 0 }}>
            <div
              className="ell"
              style={{ fontWeight: selected ? 650 : 550, minWidth: 0, fontSize: 13.5 }}
              title={a.name}
            >
              {prettyAppName(a.name, a.source)}
            </div>
            {a.publisher && (
              <div className="ell" style={{ fontSize: 11.5, color: "var(--muted)", maxWidth: 360 }}>
                {a.publisher}
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
          paddingTop: 10,
          paddingBottom: 10,
          color: sizeText === "—" ? "var(--muted)" : "var(--fg)",
        }}
      >
        {sizeText}
      </td>
      <td style={{ ...css.td, width: 150, textAlign: "right" as const, paddingTop: 8, paddingBottom: 8 }}>
        <div style={{ display: "inline-flex", gap: 6, alignItems: "center" }}>
          <button
            style={{
              ...css.btnSm,
              height: 30,
              color: hasUninstall ? "var(--danger)" : "var(--muted)",
              borderColor: hasUninstall ? "var(--danger)" : "var(--border)",
              background: hasUninstall ? "var(--danger-soft)" : "transparent",
              fontWeight: 600,
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
              onSelect(x);
              onAnalyze(x);
            }}
            onForceClean={(x) => {
              onSelect(x);
              onForceClean(x);
            }}
            onIgnoreApp={(x) => {
              onSelect(x);
              onIgnoreApp(x);
            }}
            onIgnorePub={(x) => {
              onSelect(x);
              onIgnorePub(x);
            }}
          />
        </div>
      </td>
    </tr>
  );
}

export const AppRow = memo(AppRowImpl);
