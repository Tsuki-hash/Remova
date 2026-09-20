import type { InstalledApp } from "../types";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { prettyAppName, prettyPublisher, sourceLabel } from "../lib/format";

/** Compact selected-app strip shown above the scan/list when no leftover preview. */
export function SelectedAppCard({
  selected,
  showDetail,
  onToggleDetail,
}: {
  selected: InstalledApp;
  showDetail: boolean;
  onToggleDetail: () => void;
}) {
  const L = t();
  return (
    <div
      style={{
        ...css.card,
        marginBottom: 10,
        padding: showDetail ? "10px 14px" : "8px 14px",
        fontSize: 12.5,
        background: "var(--surface-2)",
        flexShrink: 0,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <strong style={{ fontSize: 13.5 }}>{prettyAppName(selected.name, selected.source)}</strong>
        <span style={css.sourceBadge} title={selected.source}>
          {sourceLabel(selected.source, L)}
        </span>
        <button
          style={{ ...css.btnSm, height: 26, marginLeft: "auto" }}
          onClick={onToggleDetail}
          aria-expanded={showDetail}
        >
          {showDetail ? L.detailHide : L.detailShow}
        </button>
      </div>
      {showDetail && (
        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: "6px 18px",
            alignItems: "center",
            marginTop: 8,
          }}
        >
          <span className="ell" style={{ color: "var(--muted)", maxWidth: 280 }} title={selected.publisher}>
            {L.detailPublisher}: {prettyPublisher(selected.publisher)}
          </span>
          <span style={{ color: "var(--muted)", fontFamily: "var(--mono)" }}>
            {L.detailVersion}: {selected.version || "—"}
          </span>
          <span style={{ color: "var(--muted)", fontFamily: "var(--mono)" }}>
            {L.detailDate}: {selected.install_date || "—"}
          </span>
          <span
            className="ell"
            style={{
              color: "var(--muted)",
              fontFamily: "var(--mono)",
              flex: "1 1 200px",
              minWidth: 0,
            }}
            title={selected.install_location}
          >
            {selected.install_location || "—"}
          </span>
        </div>
      )}
    </div>
  );
}
