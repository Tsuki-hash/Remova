import { useEffect, useRef, useState } from "react";
import type { InstalledApp, ScanResult } from "../types";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { AppIcon } from "./AppIcon";
import { prettyAppName, sourceLabel } from "../lib/format";
import { summarizeLeftovers } from "../lib/decision";
import { buildLinkedBuckets, linkedBucketIcon } from "../lib/linkedItems";
import type { LinkedBucketId } from "../lib/linkedItems";

export type UninstallMode = "official" | "deep" | "force";

type Props = {
  app: InstalledApp;
  sizeText: string;
  uninstalling: boolean;
  scan?: ScanResult | null;
  onClose: () => void;
  onDeepUninstall: (app: InstalledApp) => void;
  onAnalyze: (app: InstalledApp) => void;
  onOfficialOnly: (app: InstalledApp) => void;
  onForceClean?: (app: InstalledApp) => void;
  onOpenPath?: (path: string) => void;
  onDrillDown?: (bucket: LinkedBucketId) => void;
  onViewLeftovers?: () => void;
};

/** Fixed right-hand detail column (not a modal drawer). */
export function AppDetailPanel({
  app,
  sizeText,
  uninstalling,
  scan,
  onClose,
  onDeepUninstall,
  onAnalyze,
  onOfficialOnly,
  onForceClean,
  onOpenPath,
  onDrillDown,
  onViewLeftovers,
}: Props) {
  const L = t();
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const name = prettyAppName(app.name, app.source);
  const hasCmd = Boolean((app.quiet_uninstall_string || app.uninstall_string || "").trim());
  const linked = scan && scan.app_name === app.name ? summarizeLeftovers(scan.items) : null;
  const buckets =
    scan && scan.app_name === app.name ? buildLinkedBuckets(scan.items, app) : [];

  useEffect(() => {
    if (!menuOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [menuOpen]);

  const runDeep = () => onDeepUninstall(app);

  const infoRows: { label: string; value: string; mono?: boolean; action?: boolean }[] = [
    { label: L.detailPath, value: app.install_location || "—", mono: true, action: true },
    { label: L.colSize, value: sizeText, mono: true },
    { label: L.detailDate, value: app.install_date || "—" },
    { label: L.detailVersion, value: app.version || "—" },
  ];

  const menuItems: { label: string; onClick: () => void; danger?: boolean }[] = [
    { label: L.drawerOfficial, onClick: () => onOfficialOnly(app) },
    { label: L.drawerDeepUninstall, onClick: runDeep },
    { label: L.rowForceClean, onClick: () => onForceClean?.(app), danger: true },
    { label: L.drawerAnalyze, onClick: () => onAnalyze(app) },
  ];

  return (
    <aside style={css.panelShell} aria-label={name}>
      <div style={css.panelHeader}>
        <AppIcon displayIcon={app.display_icon} name={app.name} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontWeight: 700, fontSize: 14.5, lineHeight: 1.3 }}>{name}</div>
          <div className="ell" style={{ ...css.muted, marginTop: 2 }}>
            {app.version || "—"}
          </div>
          <div className="ell" style={{ ...css.muted }}>
            {app.publisher || "—"}
          </div>
        </div>
        <button
          style={{ ...css.btnGhost, height: 28, width: 28, padding: 0 }}
          onClick={onClose}
          aria-label={L.cancel}
        >
          ×
        </button>
      </div>

      <div style={css.panelBody}>
        <div style={{ display: "flex", gap: 8, marginBottom: 6, alignItems: "center" }}>
          <button
            style={{
              ...css.btn,
              flex: 1,
              height: 36,
              background: "var(--accent)",
              color: "var(--accent-ink)",
              opacity: !hasCmd || uninstalling ? 0.5 : 1,
            }}
            disabled={!hasCmd || uninstalling}
            onClick={runDeep}
          >
            {uninstalling ? L.uninstalling : L.drawerDeepUninstall}
          </button>
          <button
            style={{ ...css.btnGhost, height: 36 }}
            disabled={!app.install_location}
            title={app.install_location}
            onClick={() => app.install_location && onOpenPath?.(app.install_location)}
          >
            {L.openLocation}
          </button>
          <div ref={menuRef} style={{ position: "relative" }}>
            <button
              style={{ ...css.btnGhost, height: 36, width: 36, padding: 0 }}
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen((v) => !v)}
            >
              ⋯
            </button>
            {menuOpen && (
              <div
                role="menu"
                style={{
                  position: "absolute",
                  right: 0,
                  top: 40,
                  zIndex: 20,
                  minWidth: 148,
                  background: "var(--surface)",
                  border: "1px solid var(--border)",
                  borderRadius: 8,
                  boxShadow: "0 8px 24px rgba(0,0,0,.16)",
                  padding: 4,
                }}
              >
                {menuItems.map((m) => (
                  <button
                    key={m.label}
                    role="menuitem"
                    style={{
                      display: "block",
                      width: "100%",
                      textAlign: "left",
                      border: "none",
                      background: "transparent",
                      padding: "8px 10px",
                      borderRadius: 6,
                      cursor: "pointer",
                      fontSize: 12.5,
                      color: m.danger ? "var(--danger)" : "var(--ink)",
                    }}
                    onClick={() => {
                      setMenuOpen(false);
                      m.onClick();
                    }}
                  >
                    {m.label}
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
        <div
          style={{
            marginBottom: 12,
            fontSize: 11.5,
            color: "var(--muted)",
            lineHeight: 1.45,
          }}
        >
          {L.deepUninstallExpect}
        </div>

        <div style={css.sectionTitle}>{L.basicInfo}</div>
        <div style={{ ...css.card, padding: "4px 10px", marginBottom: 14 }}>
          {infoRows.map((r) => (
            <div key={r.label} style={css.detailRow}>
              <span style={{ ...css.muted, width: 64, flexShrink: 0 }}>{r.label}</span>
              <span
                className={r.mono ? "ell" : undefined}
                style={{
                  flex: 1,
                  minWidth: 0,
                  fontFamily: r.mono ? "var(--mono)" : undefined,
                  fontSize: r.mono ? 11.5 : 12.5,
                }}
                title={r.value}
              >
                {r.value}
              </span>
              {r.action && app.install_location && (
                <button
                  style={{ ...css.btnGhost, height: 24, padding: "0 8px", fontSize: 11 }}
                  onClick={() => onOpenPath?.(app.install_location)}
                >
                  ⧉
                </button>
              )}
            </div>
          ))}
          <div style={{ padding: "7px 0", fontSize: 12, color: "var(--muted)" }}>
            {L.detailSource}: {sourceLabel(app.source, L)}
          </div>
        </div>

        <div style={css.sectionTitle}>{L.linkedItems}</div>
        {buckets.length === 0 ? (
          <div style={{ ...css.muted, marginBottom: 12, fontSize: 12, lineHeight: 1.5 }}>
            {L.linkedNotScanned || L.drawerAnalyzeHint}
            <div style={{ marginTop: 8 }}>
              <button style={{ ...css.btnGhost, height: 30 }} onClick={() => onAnalyze(app)}>
                {L.drawerAnalyze}
              </button>
            </div>
          </div>
        ) : (
          <div style={{ ...css.card, padding: "4px 10px", marginBottom: 12 }}>
            {buckets.map((b) => {
              const label =
                b.id === "programFiles"
                  ? L.linkedProgramFiles
                  : b.id === "configFiles"
                    ? L.linkedConfigFiles
                    : b.id === "registry"
                      ? L.linkedRegistry
                      : b.id === "shortcuts"
                        ? L.linkedShortcuts
                        : b.id === "startup"
                          ? L.linkedStartup
                          : L.linkedOther;
              const icon = linkedBucketIcon(b.id);
              return (
                <button
                  key={b.id}
                  type="button"
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    width: "100%",
                    padding: "8px 0",
                    border: "none",
                    borderBottom: "1px solid var(--border)",
                    fontSize: 12.5,
                    background: "transparent",
                    cursor: onDrillDown ? "pointer" : "default",
                    textAlign: "left",
                    color: "var(--ink)",
                  }}
                  onClick={() => onDrillDown?.(b.id)}
                >
                  <span style={{ width: 18, flexShrink: 0 }} aria-hidden>
                    {icon}
                  </span>
                  <span style={{ flex: 1 }}>{label}</span>
                  <span style={{ fontFamily: "var(--mono)", color: "var(--muted)" }}>
                    {b.sizeKb != null ? formatSize(b.sizeKb) : L.itemCount(b.count)}
                  </span>
                  <span style={{ color: "var(--muted)" }}>›</span>
                </button>
              );
            })}
          </div>
        )}

        {linked && linked.total > 0 ? (
          <div
            style={{
              ...css.card,
              padding: "12px",
              marginBottom: 8,
              background: "var(--surface-2)",
              borderColor: "var(--accent)",
              borderWidth: 1,
              borderStyle: "solid",
            }}
          >
            <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 6 }}>
              ✓ {L.deepUninstallRecommend}
            </div>
            <div style={{ fontSize: 12.5, color: "var(--muted)", marginBottom: 10, lineHeight: 1.45 }}>
              {L.foundNLeftovers(linked.total)}
            </div>
            <button
              style={{ ...css.btn, height: 32, width: "100%" }}
              onClick={() => onViewLeftovers?.()}
            >
              {L.viewDetails}
            </button>
          </div>
        ) : (
          <button
            style={{ ...css.btn, height: 34, width: "100%", marginBottom: 8 }}
            onClick={() => onAnalyze(app)}
          >
            {L.scanLinkedLeftovers}
          </button>
        )}
      </div>
    </aside>
  );
}
