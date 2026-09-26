import { useCallback, useRef } from "react";
import { Deco } from "./ui/Glyph";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { InstalledApp } from "../types";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { AppRow } from "./AppRow";
import type { SortCol, CategoryId } from "../lib/categories";

export function SoftwareListTable({
  filtered,
  loading,
  q,
  category,
  sortCol,
  sortDesc,
  selected,
  multi,
  uninstallingKey,
  appKey,
  sizeText,
  sizeOf,
  sortBy,
  selectApp,
  startUninstall,
  analyze,
  forceClean,
  doIgnoreApp,
  doIgnorePublisher,
  toggleMulti,
  setSelected,
}: {
  filtered: InstalledApp[];
  loading: boolean;
  q: string;
  category: CategoryId;
  sortCol: SortCol;
  sortDesc: boolean;
  selected: InstalledApp | null;
  multi: Set<string>;
  uninstallingKey: string | null;
  appKey: (a: InstalledApp) => string;
  sizeText: (a: InstalledApp) => string;
  sizeOf: (a: InstalledApp) => number;
  sortBy: (col: "name" | "size" | "recommend") => void;
  selectApp: (a: InstalledApp) => void;
  startUninstall: (a: InstalledApp) => void;
  analyze: (a: InstalledApp) => void;
  forceClean: (a: InstalledApp) => void;
  doIgnoreApp: (a: InstalledApp) => void;
  doIgnorePublisher: (a: InstalledApp) => void;
  toggleMulti: (key: string) => void;
  setSelected: (a: InstalledApp | null) => void;
}) {
  const L = t();
  const listScrollRef = useRef<HTMLDivElement | null>(null);
  // F-R6-10: read `selected` via ref so `ensureSelected` stays referentially stable
  // and does not punch a hole through AppRow's memo when selection changes.
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const ensureSelected = useCallback(
    (app: InstalledApp) => {
      if (!selectedRef.current) setSelected(app);
    },
    [setSelected],
  );
  const rowVirtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => listScrollRef.current,
    estimateSize: () => 56,
    overscan: 12,
    getItemKey: (index) => filtered[index] ? appKey(filtered[index]) : index,
  });
  const virtualRows = rowVirtualizer.getVirtualItems();
  const totalSize = rowVirtualizer.getTotalSize();

  const renderRow = (a: InstalledApp, index: number) => {
    const key = appKey(a);
    return (
      <AppRow
        key={key}
        index={index}
        rowRef={rowVirtualizer.measureElement}
        app={a}
        rowKey={key}
        selected={selected?.registry_key === a.registry_key && selected.name === a.name}
        checked={multi.has(key)}
        sizeText={sizeText(a)}
        sizeKb={sizeOf(a)}
        uninstalling={uninstallingKey === key}
        onSelect={selectApp}
        onUninstall={startUninstall}
        onAnalyze={analyze}
        onForceClean={forceClean}
        onIgnoreApp={doIgnoreApp}
        onIgnorePub={doIgnorePublisher}
        onToggleMulti={toggleMulti}
        onEnsureSelected={ensureSelected}
      />
    );
  };

  return (
    <div ref={listScrollRef} style={css.scroll}>
      {loading && filtered.length === 0 && (
        <div style={{ padding: "12px 16px", display: "flex", flexDirection: "column", gap: 10 }}>
          {Array.from({ length: 8 }).map((_, i) => (
            <div key={i} className="remova-skeleton" style={{ height: 48, opacity: 1 - i * 0.08 }} />
          ))}
        </div>
      )}
      <table style={css.table}>
        <colgroup>
          <col style={{ width: 40 }} />
          <col />
          <col style={{ width: 100 }} />
          <col style={{ width: 150 }} />
        </colgroup>
        <thead>
          <tr>
            <th style={css.th} title="selected"><Deco ch="✓" label="selected" /></th>
            <th
              style={{ ...css.th, cursor: "pointer" }}
              onClick={() => sortBy("name")}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  sortBy("name");
                }
              }}
              tabIndex={0}
              aria-sort={sortCol === "name" ? (sortDesc ? "descending" : "ascending") : undefined}
              title={L.colName}
            >
              {L.colName} {sortCol === "name" ? (sortDesc ? "↓" : "↑") : ""}
            </th>
            <th
              style={{ ...css.th, cursor: "pointer", textAlign: "right" as const }}
              onClick={() => sortBy("size")}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  sortBy("size");
                }
              }}
              tabIndex={0}
              aria-sort={sortCol === "size" ? (sortDesc ? "descending" : "ascending") : undefined}
            >
              {L.colSize} {sortCol === "size" ? (sortDesc ? "↓" : "↑") : ""}
            </th>
            <th style={{ ...css.th, textAlign: "right" as const }}>{L.actionCol}</th>
          </tr>
        </thead>
        <tbody>
          {filtered.length === 0 && !loading && (
            <tr>
              <td
                colSpan={4}
                style={{ ...css.td, color: "var(--muted)", textAlign: "center" as const, padding: 28 }}
              >
                {loading
                  ? L.loadingApps
                  : q.trim() || category !== "all"
                    ? L.emptySearch
                    : L.emptyList}
              </td>
            </tr>
          )}
          {virtualRows.length > 0 && (
            <>
              {virtualRows[0] && virtualRows[0].start > 0 && (
                <tr aria-hidden style={{ height: virtualRows[0].start }}>
                  <td colSpan={4} style={{ padding: 0, border: "none" }} />
                </tr>
              )}
              {virtualRows.map((vr) => {
                const app = filtered[vr.index];
                return app ? renderRow(app, vr.index) : null;
              })}
              {(() => {
                const last = virtualRows[virtualRows.length - 1];
                if (!last) return null;
                const pad = totalSize - last.end;
                return pad > 0 ? (
                  <tr aria-hidden style={{ height: pad }}>
                    <td colSpan={4} style={{ padding: 0, border: "none" }} />
                  </tr>
                ) : null;
              })()}
            </>
          )}
        </tbody>
      </table>
    </div>
  );
}
