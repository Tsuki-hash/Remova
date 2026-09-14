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
        background: selected ? "var(--row-selected)" : undefined,
        boxShadow: selected ? "inset 2px 0 0 var(--accent)" : undefined,
      }}
    >
      <td style={{ ...css.td, width: 36, textAlign: "center" as const }}>
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
        />
      </td>
      <td style={css.td}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          <AppIcon displayIcon={a.display_icon} name={a.name} />
          <span
            className="ell"
            style={{ fontWeight: selected ? 600 : 500, minWidth: 0 }}
            title={a.name}
          >
            {prettyAppName(a.name, a.source)}
          </span>
        </div>
      </td>
      <td style={{ ...css.td, ...css.mono, whiteSpace: "nowrap" }}>
        {a.version || "—"}
      </td>
      <td style={css.td}>
        <span className="ell" style={{ display: "block", minWidth: 0 }} title={a.publisher}>
          {prettyPublisher(a.publisher)}
        </span>
      </td>
      <td
        style={{
          ...css.td,
          ...css.mono,
          textAlign: "right" as const,
          whiteSpace: "nowrap",
          color: sizeText === "—" ? "var(--muted)" : "var(--fg)",
        }}
      >
        {sizeText}
      </td>
      <td style={{ ...css.td, ...css.mono, whiteSpace: "nowrap", color: "var(--muted)" }}>
        {a.install_date || "—"}
      </td>
      <td style={{ ...css.td, textAlign: "center" as const }}>
        <span style={css.sourceBadge}>{a.source}</span>
      </td>
      <td style={css.td}>
        <span
          className="ell"
          style={{
            display: "block",
            color: "var(--muted)",
            fontSize: 12,
            fontFamily: "var(--mono)",
            minWidth: 0,
          }}
          title={a.install_location}
        >
          {a.install_location ? shortPath(a.install_location) : "—"}
        </span>
      </td>
    </tr>
  );
}

export const AppRow = memo(AppRowImpl);
