import { memo } from "react";
import type { InstalledApp } from "../types";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { AppIcon } from "./AppIcon";
import { prettyAppName, prettyPublisher, shortPath } from "../lib/format";

type Props = {
  app: InstalledApp;
  rowKey: string;
  selected: boolean;
  checked: boolean;
  sizeText: string;
  onSelect: (app: InstalledApp) => void;
  onAnalyze: (app: InstalledApp) => void;
  onToggleMulti: (key: string) => void;
  onEnsureSelected: (app: InstalledApp) => void;
};

function AppRowImpl({
  app,
  rowKey,
  selected,
  checked,
  sizeText,
  onSelect,
  onAnalyze,
  onToggleMulti,
  onEnsureSelected,
}: Props) {
  const L = t();
  const a = app;
  return (
    <tr
      onClick={() => onSelect(a)}
      onDoubleClick={() => onAnalyze(a)}
      title={[
        prettyAppName(a.name, a.source),
        a.publisher && `${L.colPublisher}: ${a.publisher}`,
        a.install_location && `${L.colLocation}: ${a.install_location}`,
        (a.quiet_uninstall_string || a.uninstall_string) &&
          `${L.colUninstall}: ${a.quiet_uninstall_string || a.uninstall_string}`,
        a.registry_key && `${L.colRegKey}: ${a.registry_key}`,
      ]
        .filter(Boolean)
        .join("\n") || undefined}
      style={{
        cursor: "pointer",
        background: selected ? "var(--th-bg)" : undefined,
      }}
    >
      <td style={css.td}>
        <input
          type="checkbox"
          checked={checked}
          onChange={() => {
            const next = !checked;
            onToggleMulti(rowKey);
            if (next && !selected) onEnsureSelected(a);
          }}
          onClick={(e) => e.stopPropagation()}
        />
      </td>
      <td style={css.td}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          <AppIcon displayIcon={a.display_icon} name={a.name} />
          <span className="ell" style={{ maxWidth: 280 }} title={a.name}>
            {prettyAppName(a.name, a.source)}
          </span>
        </div>
      </td>
      <td style={{ ...css.td, whiteSpace: "nowrap", color: "var(--muted)" }}>
        {a.version || "—"}
      </td>
      <td style={css.td}>
        <span className="ell" style={{ display: "block", maxWidth: 180 }} title={a.publisher}>
          {prettyPublisher(a.publisher)}
        </span>
      </td>
      <td
        style={{
          ...css.td,
          textAlign: "right" as const,
          whiteSpace: "nowrap",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {sizeText}
      </td>
      <td style={{ ...css.td, whiteSpace: "nowrap", color: "var(--muted)" }}>
        {a.install_date || "—"}
      </td>
      <td style={css.td}>
        <span
          style={{
            display: "inline-block",
            padding: "2px 8px",
            borderRadius: 6,
            fontSize: 11,
            background: a.source === "Store" ? "var(--surface-2)" : "transparent",
            border: "1px solid var(--border)",
          }}
        >
          {a.source}
        </span>
      </td>
      <td style={css.td}>
        <span
          className="ell"
          style={{ display: "block", maxWidth: 320, color: "var(--muted)", fontSize: 12 }}
          title={a.install_location}
        >
          {a.install_location ? shortPath(a.install_location) : "—"}
        </span>
      </td>
    </tr>
  );
}

export const AppRow = memo(AppRowImpl);
