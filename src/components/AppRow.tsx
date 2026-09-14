import { memo } from "react";
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
  onToggleMulti: (key: string) => void;
  onEnsureSelected: (app: InstalledApp) => void;
};

function AppRowImpl({
  app,
  rowKey,
  selected,
  checked,
  sizeText,
  uninstalling,
  onSelect,
  onUninstall,
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
      <td style={{ ...css.td, width: 40, textAlign: "center" as const, paddingTop: 10, paddingBottom: 10 }}>
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
        <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
          <AppIcon displayIcon={a.display_icon} name={a.name} />
          <span
            className="ell"
            style={{ fontWeight: selected ? 650 : 550, minWidth: 0, fontSize: 13.5 }}
            title={a.name}
          >
            {prettyAppName(a.name, a.source)}
          </span>
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
      <td style={{ ...css.td, width: 108, textAlign: "right" as const, paddingTop: 8, paddingBottom: 8 }}>
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
      </td>
    </tr>
  );
}

export const AppRow = memo(AppRowImpl);
