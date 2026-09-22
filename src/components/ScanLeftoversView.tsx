import { memo, useMemo, useRef, type ReactNode } from "react";
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
import type { LinkedBucketId } from "../lib/linkedItems";
import type { CleanupItem, InstalledApp, ScanResult } from "../types";

type LeftoverRowProps = {
  it: CleanupItem;
  checked: boolean;
  note: string | undefined;
  onTogglePath: (path: string) => void;
  onEvidence: (text: string | null) => void;
};

/** F-R6-10: memoized row so virtual-list parent re-renders skip unchanged items. */
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
        gridTemplateColumns: "28px 1fr 120px 90px 40px",
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
          checked={checked}
          onChange={() => onTogglePath(it.path)}
        />
      </div>
      <div style={{ minWidth: 0 }}>
        <span className="ell" style={{ display: "block", fontSize: 12.5 }} title={it.path}>
          {it.path}
        </span>
        <div style={{ fontSize: 11.5, color: "var(--muted)", marginTop: 2, lineHeight: 1.4 }}>
          {reasonLine}
          {it.reason && it.reason !== reasonLine ? ` · ${it.reason}` : ""}
        </div>
        {note && (
          <div
            style={{
              fontSize: 11.5,
              color: "var(--accent)",
              marginTop: 2,
              lineHeight: 1.4,
              fontWeight: 500,
            }}
          >
            ✦ {note}
            <span style={{ opacity: 0.75, color: "var(--muted)" }}> · {L.aiDisclaimer}</span>
          </div>
        )}
      </div>
      <div>
        <span style={css.sourceBadge}>{it.kind}</span>
        {it.shared && (
          <span
            style={{
              ...css.sourceBadge,
              marginLeft: 6,
              color: "var(--warn)",
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
              color: "var(--danger)",
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
              color: "var(--mid, #FFB020)",
              borderColor: "var(--mid, #FFB020)",
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
              ? "var(--danger)"
              : it.confidence === "confirmed"
                ? "var(--ok)"
                : "var(--warn)",
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
          onClick={() =>
            onEvidence(
              it.evidence
                .map((e) => `${e.label} (${e.weight})${e.detail ? " — " + e.detail : ""}`)
                .join("\n") || it.reason,
            )
          }
        >
          ⓘ
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
  /** Decision-layer filters from CleanupConclusion. */
  riskFilter?: "confirm" | "keep" | null;
  onClearRiskFilter?: () => void;
  conclusion?: ReactNode;
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
  onClearRiskFilter,
  conclusion,
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
  const filterCounts = useMemo(() => summarizeLeftovers(bucketItems), [bucketItems]);
  const filterLabel = useMemo(() => {
    if (!kindFilter) return null;
    const key = linkedBucketLabelKey(kindFilter);
    return L[key];
  }, [kindFilter, L]);

  const rowVirtualizer = useVirtualizer({
    count: displayItems.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 72,
    overscan: 8,
    getItemKey: (index) => displayItems[index]?.path ?? index,
  });
  const virtualRows = rowVirtualizer.getVirtualItems();
  const totalSize = rowVirtualizer.getTotalSize();

  return (
    <div style={{ ...css.card, flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
      <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
        <strong>{scan.app_name}</strong>
        <span style={{ ...css.muted, marginLeft: 12 }}>
          {L.leftoversTitle}: {scan.items.length} ·{" "}
          {scan.items.filter((i) => i.confidence === "confirmed").length} {L.confirmed}
        </span>
      </div>
      <LeftoverSummaryBar summary={summarizeLeftovers(scan.items)} scanning={scanning} />
      {conclusion}
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
      {!isOrphan && riskFilter && (
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
          <span style={css.chip}>
            {riskFilter === "keep"
              ? L.conclusionShowKeep(filterCounts.keep)
              : L.conclusionShowConfirm(filterCounts.suggest)}
          </span>
          <button style={{ ...css.btnGhost, height: 26 }} onClick={onClearRiskFilter}>
            {L.showAllLeftovers}
          </button>
        </div>
      )}
      <div
        ref={scrollRef}
        style={{ ...css.scroll, position: "relative" }}
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
            gridTemplateColumns: "28px 1fr 120px 90px 40px",
            gap: 8,
            padding: "8px 14px",
            borderBottom: "1px solid var(--border)",
          }}
        >
          <span>✓</span>
          <span>{L.colLocation}</span>
          <span>{L.colSource}</span>
          <span>{L.colConfidence}</span>
          <span>ⓘ</span>
        </div>
        {displayItems.length === 0 ? (
          <div style={{ padding: 16, color: "var(--muted)", fontSize: 12 }}>{L.leftoversNone}</div>
        ) : (
          <div style={{ height: totalSize, position: "relative" }}>
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
                    transform: `translateY(${vr.start}px)`,
                  }}
                >
                  <LeftoverRow
                    it={it}
                    checked={selectedPaths.has(it.path)}
                    note={aiNotes[it.path]}
                    onTogglePath={onTogglePath}
                    onEvidence={onEvidence}
                  />
                </div>
              );
            })}
          </div>
        )}
      </div>
      {evidence && (
        <div
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
          <button style={{ ...css.btnGhost, height: 28 }} onClick={() => onEvidence(null)}>
            ×
          </button>
        </div>
      )}
    </div>
  );
}
