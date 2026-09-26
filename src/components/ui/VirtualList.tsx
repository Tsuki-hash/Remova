import { useRef, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

/**
 * REV-UX-18: shared windowed list for the tool panels (History / Disk / Idle).
 * Rows are absolutely positioned and measured, so variable heights stay correct
 * while only the visible window renders.
 */
export function VirtualList<T>({
  items,
  keyOf,
  renderItem,
  empty,
  height = 320,
  estimateSize = 64,
  rowGap = 6,
}: {
  items: T[];
  keyOf: (item: T, index: number) => string;
  renderItem: (item: T, index: number) => ReactNode;
  empty?: ReactNode;
  height?: number;
  estimateSize?: number;
  rowGap?: number;
}) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => estimateSize,
    overscan: 8,
    getItemKey: (i) => keyOf(items[i]!, i),
  });
  if (items.length === 0) {
    return empty ? <>{empty}</> : null;
  }
  const virtualItems = virtualizer.getVirtualItems();
  // Unmeasurable viewport (jsdom, display:none) yields an empty window —
  // render everything rather than nothing. Real browsers always measure.
  const fallbackAll = virtualItems.length === 0;
  return (
    <div ref={scrollRef} style={{ maxHeight: height, overflow: "auto", marginTop: 10 }}>
      <div style={{ height: fallbackAll ? undefined : virtualizer.getTotalSize(), position: "relative" }}>
        {fallbackAll
          ? items.map((it, i) => (
              <div key={keyOf(it, i)} style={{ paddingBottom: rowGap }}>
                {renderItem(it, i)}
              </div>
            ))
          : virtualItems.map((vr) => (
              <div
                key={vr.key}
                data-index={vr.index}
                ref={virtualizer.measureElement}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${vr.start}px)`,
                  paddingBottom: rowGap,
                }}
              >
                {renderItem(items[vr.index]!, vr.index)}
              </div>
            ))}
      </div>
    </div>
  );
}
