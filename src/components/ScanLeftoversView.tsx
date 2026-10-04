import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { CloseGlyph, Deco } from "./ui/Glyph";
import { useVirtualizer } from "@tanstack/react-virtual";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { LeftoverSummaryBar } from "./LeftoverSummaryBar";
import { OrphanOriginGroups } from "./OrphanOriginGroups";
import {
  flattenOriginGroups,
  groupByOrigin,
  isKeepItem,
  isSuggestItem,
  leftoverReasonLine,
  summarizeLeftovers,
} from "../lib/decision";
import { filterItemsByBucket, linkedBucketLabelKey } from "../lib/linkedItems";
import { backendText } from "../lib/backendText";
import { evidenceText } from "../lib/evidenceText";
import type { LinkedBucketId } from "../lib/linkedItems";
import type { CleanupItem, InstalledApp, ScanResult } from "../types";

type LeftoverRowProps = {
  it: CleanupItem;
  checked: boolean;
  note: string | undefined;
  onTogglePath: (path: string) => void;
  onEvidence: (text: string | null) => void;
};

/** -10: memoized row so virtual-list parent re-renders skip unchanged items. */
const LeftoverRow = memo(function LeftoverRow({
  it,
  checked,
  note,
  onTogglePath,
  onEvidence,
}: LeftoverRowProps) {
  const L = t();
  const reasonLine = leftoverReasonLine(it, L);
  const noted = Boolean(note);
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "28px minmax(0, 1fr) 120px 90px 40px",
        gap: 8,
        alignItems: "start",
        padding: "10px 14px",
        borderBottom: "1px solid var(--border)",
        boxShadow: it.risk === "high"
          ? "inset 3px 0 0 var(--danger)"
          : noted
            ? "inset 3px 0 0 var(--accent)"
            : undefined,
        background: noted ? "var(--accent-soft)" : undefined,
      }}
    >
      <div>
        <input
          type="checkbox"
          aria-label={it.path}
          checked={checked}
          onChange={() => onTogglePath(it.path)}
        />
      </div>
      <div style={{ minWidth: 0, overflow: "hidden" }}>
        <span
          className="ell"
          style={{
            display: "block",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            fontSize: 12.5,
          }}
          title={it.path}
        >
          {it.path}
        </span>
        <div style={{ fontSize: 11.5, color: "var(--muted)", marginTop: 2, lineHeight: 1.4 }}>
          {reasonLine}
          {it.reason ? ` · ${backendText(it.reason, "scan")}` : ""}
        </div>
        {note && (
          <div
            style={{
              fontSize: 11.5,
              color: "var(--accent-text)",
              marginTop: 2,
              lineHeight: 1.4,
              fontWeight: 500,
            }}
          >
            <Deco ch="✦" /> {note}
            <span style={{ color: "var(--muted)" }}> · {L.aiDisclaimer}</span>
          </div>
        )}
      </div>
      <div>
        <span style={css.sourceBadge}>{L.kindLabel(it.kind)}</span>
        {it.shared && (
          <span
            style={{
              ...css.sourceBadge,
              marginLeft: 6,
              color: "var(--warn-ink)",
              borderColor: "var(--warn)",
            }}
            title={L.sharedHint}
          >
            {L.badgeShared}
          </span>
        )}
        {it.user_data && (
          <span
            style={{
              ...css.sourceBadge,
              marginLeft: 6,
              color: "var(--danger-text)",
              borderColor: "var(--danger)",
            }}
            title={L.userDataHint}
          >
            {L.badgeUserData}
          </span>
        )}
        {it.user_library && (
          <span
            style={{
              ...css.sourceBadge,
              marginLeft: 6,
 // `--mid` never existed — the fallback amber was
 // unreadable on light panels as text (≈1.75:1).
              color: "var(--warn-ink)",
              borderColor: "var(--warn)",
            }}
            title={L.userLibraryHint}
          >
            {L.badgeUserLibrary}
          </span>
        )}
      </div>
      <div
        style={{
          color:
            it.risk === "high"
              ? "var(--danger-text)"
              : it.confidence === "confirmed"
                ? "var(--ok-ink)"
                : "var(--warn-ink)",
          fontWeight: 600,
          fontSize: 12,
        }}
      >
        {it.risk === "high"
          ? L.riskHigh
          : it.risk === "medium"
            ? L.riskMedium
            : it.confidence === "confirmed"
              ? L.confirmed
              : it.score >= 30
                ? L.suspected
                : L.low}
      </div>
      <div>
        <button
          style={{ ...css.btnGhost, height: 28, width: 32, padding: 0 }}
          aria-label={`${L.orphanEvidenceTitle}: ${it.path}`}
          onClick={() =>
            onEvidence(
              it.evidence
                .map(raw => ({ ...raw, ...evidenceText(raw) }))
                .map((e) => `${e.label} (${e.weight})${e.detail ? " — " + e.detail : ""}`)
                .join("\n") || it.reason,
            )
          }
        >
          <Deco ch="ⓘ" />
        </button>
      </div>
    </div>
  );
});

type Props = {
  scan: ScanResult;
  scanning: boolean;
  selectedPaths: Set<string>;
  evidence: string | null;
  aiNotes: Record<string, string>;
  orphanLabel: string;
  onTogglePath: (path: string) => void;
  onEvidence: (text: string | null) => void;
  /** Filter leftovers by linked bucket (detail-panel drill-down). */
  kindFilter?: LinkedBucketId | null;
  filterApp?: InstalledApp | null;
  onClearKindFilter?: () => void;
  riskFilter?: "confirm" | "keep" | null;
  onSelectSafe?: () => void;
  onShowConfirm?: () => void;
  onShowKeep?: () => void;
};

/** Leftover list + summary + optional orphan origin groups (virtualized). */
export function ScanLeftoversView({
  scan,
  scanning,
  selectedPaths,
  evidence,
  aiNotes,
  orphanLabel,
  onTogglePath,
  onEvidence,
  kindFilter,
  filterApp,
  onClearKindFilter,
  riskFilter,
  onSelectSafe,
  onShowConfirm,
  onShowKeep,
}: Props) {
  const L = t();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const isOrphan = scan.app_name === orphanLabel;
  const originGroups = useMemo(
    () => (isOrphan ? groupByOrigin(scan.items) : []),
    [isOrphan, scan.items],
  );
  const baseItems = useMemo(
    () => (isOrphan ? flattenOriginGroups(originGroups) : scan.items),
    [isOrphan, originGroups, scan.items],
  );
  const bucketItems = useMemo(
    () =>
      !isOrphan && kindFilter && filterApp
        ? filterItemsByBucket(baseItems, filterApp, kindFilter)
        : baseItems,
    [isOrphan, kindFilter, filterApp, baseItems],
  );
  const displayItems = useMemo(
    () =>
      !riskFilter
        ? bucketItems
        : bucketItems.filter((it) =>
            riskFilter === "keep" ? isKeepItem(it) : isSuggestItem(it),
          ),
    [riskFilter, bucketItems],
  );
  const filterLabel = useMemo(() => {
    if (!kindFilter) return null;
    const key = linkedBucketLabelKey(kindFilter);
    return L[key];
  }, [kindFilter, L]);

 // The list starts below the column header and (for orphans) the origin-group
 // block — tell the virtualizer where item 0 actually sits.
  const listRef = useRef<HTMLDivElement | null>(null);
  const [listMargin, setListMargin] = useState(0);
  const evidenceRef = useRef<HTMLDivElement | null>(null);
  // The evidence block sits below the (capped) list at the bottom of a
  // page-scrolled view — without scrolling it into view a click looks dead.
  useEffect(() => {
    if (evidence) evidenceRef.current?.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
  }, [evidence]);
  // Same row's ⓘ toggles: open, then close on a second click.
  const showEvidence = (text: string | null) => {
    if (text === null) {
      onEvidence(null);
      return;
    }
    onEvidence(evidence === text ? null : text);
  };
  useLayoutEffect(() => {
    const el = listRef.current;
    const sc = scrollRef.current;
    if (!el || !sc) return;
    const next = Math.max(
      0,
      Math.round(el.getBoundingClientRect().top - sc.getBoundingClientRect().top + sc.scrollTop),
    );
    setListMargin((m) => (Math.abs(m - next) > 0.5 ? next : m));
  }, [displayItems.length, kindFilter]);
  const rowVirtualizer = useVirtualizer({
    count: displayItems.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 72,
    overscan: 8,
    scrollMargin: listMargin,
    getItemKey: (index) => displayItems[index]?.path ?? index,
  });
  const virtualRows = rowVirtualizer.getVirtualItems();
  const totalSize = rowVirtualizer.getTotalSize();

  return (
    <div style={{ ...css.card, flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
      <LeftoverSummaryBar
        summary={summarizeLeftovers(scan.items)}
        scanning={scanning}
        riskFilter={riskFilter}
        onSelectSafe={onSelectSafe}
        onShowConfirm={onShowConfirm}
        onShowKeep={onShowKeep}
      />
      {!isOrphan && kindFilter && filterLabel && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "8px 14px",
            borderBottom: "1px solid var(--border)",
            fontSize: 12,
            flexShrink: 0,
          }}
        >
          <span style={css.chip}>{L.filterOnly(filterLabel)}</span>
          <button style={{ ...css.btnGhost, height: 26 }} onClick={onClearKindFilter}>
            {L.showAllLeftovers}
          </button>
        </div>
      )}
      <div
        ref={scrollRef}
        style={{
          ...css.scroll,
          position: "relative",
          // The page (Shell main) is the outer scroller; without a bound this
          // container grows with its content and, with `contain`, becomes a
          // wheel dead zone. Cap it so long lists scroll here (and the
          // virtualizer engages), and let wheel chaining reach the page when
          // the inner list is at its end.
          maxHeight: "60vh",
          overscrollBehavior: "auto",
        }}
      >
        {isOrphan && originGroups.length > 0 && (
          <div style={{ padding: "10px 14px 0" }}>
            <OrphanOriginGroups
              groups={originGroups}
              selectedPaths={selectedPaths}
              onToggle={onTogglePath}
            />
          </div>
        )}
        <div
          style={{
            fontWeight: 600,
            fontSize: 11,
            color: "var(--muted)",
            display: "grid",
            // minmax(0,1fr): an unbreakable long path must shrink to the
            // track (ellipsis via .ell), not push the 来源/判定 columns over.
            gridTemplateColumns: "28px minmax(0, 1fr) 120px 90px 40px",
            gap: 8,
            padding: "8px 14px",
            borderBottom: "1px solid var(--border)",
          }}
        >
          <Deco ch="✓" />
          <span>{L.colLocation}</span>
          <span>{L.colSource}</span>
          <span>{L.colConfidence}</span>
          <Deco ch="ⓘ" />
        </div>
        {displayItems.length === 0 ? (
          <div style={{ padding: 16, color: "var(--muted)", fontSize: 12 }}>{L.leftoversNone}</div>
        ) : (
          <div
            ref={listRef}
 // getTotalSize() already excludes scrollMargin (TanStack v3) —
 // subtracting listMargin here double-counted and clipped the tail.
            style={{ height: totalSize, position: "relative" }}
          >
            {virtualRows.map((vr) => {
              const it = displayItems[vr.index];
              if (!it) return null;
              return (
                <div
                  key={it.path}
                  ref={rowVirtualizer.measureElement}
                  data-index={vr.index}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    right: 0,
                    transform: `translateY(${vr.start - listMargin}px)`,
                  }}
                >
                  <LeftoverRow
                    it={it}
                    checked={selectedPaths.has(it.path)}
                    note={aiNotes[it.path]}
                    onTogglePath={onTogglePath}
                    onEvidence={showEvidence}
                  />
                </div>
              );
            })}
          </div>
        )}
      </div>
      {evidence && (
        <div
          ref={evidenceRef}
          style={{
            padding: 12,
            borderTop: "1px solid var(--border)",
            fontSize: 13,
            whiteSpace: "pre-wrap",
            background: "var(--th-bg)",
            flexShrink: 0,
          }}
        >
          {evidence}{" "}
          <button style={{ ...css.btnGhost, height: 28 }} onClick={() => onEvidence(null)} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      )}
    </div>
  );
}
