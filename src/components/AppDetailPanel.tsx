import { useState, type ReactNode } from "react";
import type { InstalledApp, ScanResult } from "../types";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { AppIcon } from "./AppIcon";
import { prettyAppName, sourceLabel } from "../lib/format";
import { appHealth, summarizeLeftovers } from "../lib/decision";

export type UninstallMode = "official" | "deep" | "force";

type Props = {
  app: InstalledApp;
  sizeText: string;
  sizeKb: number;
  uninstalling: boolean;
  scan?: ScanResult | null;
  onClose: () => void;
  onDeepUninstall: (app: InstalledApp) => void;
  onAnalyze: (app: InstalledApp) => void;
  onOfficialOnly: (app: InstalledApp) => void;
  onForceClean?: (app: InstalledApp) => void;
  onOpenPath?: (path: string) => void;
  extra?: ReactNode;
};

/** Fixed right-hand detail column (not a modal drawer). */
export function AppDetailPanel({
  app,
  sizeText,
  sizeKb,
  uninstalling,
  scan,
  onClose,
  onDeepUninstall,
  onAnalyze,
  onOfficialOnly,
  onForceClean,
  onOpenPath,
  extra,
}: Props) {
  const L = t();
  const [mode, setMode] = useState<UninstallMode>("deep");
  const name = prettyAppName(app.name, app.source);
  const health = appHealth(app, sizeKb, L);
  const hasCmd = Boolean((app.quiet_uninstall_string || app.uninstall_string || "").trim());
  const linked = scan && scan.app_name === app.name ? summarizeLeftovers(scan.items) : null;

  const startByMode = () => {
    if (mode === "official") onOfficialOnly(app);
    else if (mode === "force") onForceClean?.(app);
    else onDeepUninstall(app);
  };

  const primaryLabel =
    mode === "official"
      ? L.drawerOfficial
      : mode === "force"
        ? L.rowForceClean
        : L.drawerDeepUninstall;

  const infoRows: { label: string; value: string; mono?: boolean; action?: boolean }[] = [
    { label: L.detailPath, value: app.install_location || "—", mono: true, action: true },
    { label: L.colSize, value: sizeText, mono: true },
    { label: L.detailDate, value: app.install_date || "—" },
    { label: L.detailVersion, value: app.version || "—" },
    { label: L.detailSource, value: sourceLabel(app.source, L) },
    { label: L.healthLabel, value: health.label },
  ];

  const linkRows = linked
    ? linked.byKind.slice(0, 6).map((k) => ({ id: k.kind, label: k.kind, value: String(k.count) }))
    : [];

  return (
    <aside
      style={{
        width: 340,
        flexShrink: 0,
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: 8,
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        overflow: "hidden",
      }}
      aria-label={name}
    >
      <div
        style={{
          display: "flex",
          gap: 12,
          alignItems: "flex-start",
          padding: "14px 14px 10px",
          borderBottom: "1px solid var(--border)",
        }}
      >
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

      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "12px 14px" }}>
        <div style={{ display: "flex", gap: 8, marginBottom: 14 }}>
          <button
            style={{
              ...css.btn,
              flex: 1,
              height: 36,
              background: mode === "force" ? "var(--danger)" : "var(--accent)",
              color: "var(--accent-ink)",
              opacity: mode === "force" ? (uninstalling ? 0.5 : 1) : !hasCmd || uninstalling ? 0.5 : 1,
            }}
            disabled={mode === "force" ? uninstalling : !hasCmd || uninstalling}
            onClick={startByMode}
          >
            {uninstalling ? L.uninstalling : mode === "deep" ? L.drawerDeepUninstall : primaryLabel}
          </button>
          <button
            style={{ ...css.btnGhost, height: 36 }}
            disabled={!app.install_location}
            title={app.install_location}
            onClick={() => app.install_location && onOpenPath?.(app.install_location)}
          >
            {L.openLocation}
          </button>
          <button
            style={{ ...css.btnGhost, height: 36, width: 36, padding: 0 }}
            title={L.drawerAnalyzeHint}
            onClick={() => onAnalyze(app)}
          >
            ⋯
          </button>
        </div>

        <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 8 }}>{L.basicInfo}</div>
        <div style={{ ...css.card, padding: "4px 10px", marginBottom: 14 }}>
          {infoRows.map((r) => (
            <div
              key={r.label}
              style={{
                display: "flex",
                gap: 10,
                alignItems: "center",
                padding: "7px 0",
                borderBottom: "1px solid var(--border)",
                fontSize: 12.5,
              }}
            >
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
        </div>

        <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 8 }}>{L.linkedItems}</div>
        {linkRows.length === 0 ? (
          <div style={{ ...css.muted, marginBottom: 12, fontSize: 12 }}>
            {L.drawerAnalyzeHint}
            <div style={{ marginTop: 8 }}>
              <button style={{ ...css.btnGhost, height: 30 }} onClick={() => onAnalyze(app)}>
                {L.drawerAnalyze}
              </button>
            </div>
          </div>
        ) : (
          <div style={{ ...css.card, padding: "4px 10px", marginBottom: 12 }}>
            {linkRows.map((r) => (
              <div
                key={r.id}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  padding: "8px 0",
                  borderBottom: "1px solid var(--border)",
                  fontSize: 12.5,
                }}
              >
                <span style={{ flex: 1 }}>{r.label}</span>
                <span style={{ fontFamily: "var(--mono)", color: "var(--muted)" }}>{r.value}</span>
                <span style={{ color: "var(--muted)" }}>›</span>
              </div>
            ))}
            <div style={{ padding: "8px 0", fontSize: 12, color: "var(--muted)" }}>
              {L.bucketSafe} {linked?.safe} · {L.bucketSuggest} {linked?.suggest} · {L.bucketKeep}{" "}
              {linked?.keep}
            </div>
          </div>
        )}

        {extra}

        <div style={{ fontWeight: 700, fontSize: 13, margin: "14px 0 8px" }}>{L.uninstallModeTitle}</div>
        {(
          [
            ["official", L.modeOfficial, L.modeOfficialHint],
            ["deep", L.modeDeep, L.modeDeepHint],
            ["force", L.modeForce, L.modeForceHint],
          ] as const
        ).map(([id, label, hint]) => (
          <label
            key={id}
            style={{
              display: "flex",
              gap: 8,
              alignItems: "flex-start",
              fontSize: 12.5,
              cursor: "pointer",
              lineHeight: 1.4,
              marginBottom: 8,
            }}
          >
            <input
              type="radio"
              name="uninstall-mode"
              checked={mode === id}
              onChange={() => setMode(id)}
              style={{ marginTop: 2, accentColor: "var(--accent)" }}
            />
            <span>
              <span style={{ fontWeight: 600 }}>{label}</span>
              <span style={{ color: "var(--muted)", display: "block", fontSize: 11.5 }}>{hint}</span>
            </span>
          </label>
        ))}
      </div>
    </aside>
  );
}
