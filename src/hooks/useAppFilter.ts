import { useCallback, useMemo } from "react";
import type { InstalledApp } from "../types";
import { recommendScore } from "../lib/decision";
import type { CategoryId, SortCol } from "../lib/categories";

export type { CategoryId, SortCol };

export function useAppFilter({
  apps,
  copilotList,
  deferredQ,
  category,
  sortCol,
  sortDesc,
  sizeOf,
  ignorePub,
  ignoreName,
  sizeMap,
  setSortCol,
  setSortDesc,
}: {
  apps: InstalledApp[];
  copilotList: InstalledApp[] | null;
  deferredQ: string;
  category: CategoryId;
  sortCol: SortCol;
  sortDesc: boolean;
  sizeOf: (a: InstalledApp) => number;
  ignorePub: string[];
  ignoreName: string[];
  /** Size estimates fill async; include so large/sort recompute when they land. */
  sizeMap?: Record<string, number>;
  setSortCol: (c: SortCol) => void;
  setSortDesc: (updater: boolean | ((d: boolean) => boolean)) => void;
}) {
  const filtered = useMemo(() => {
    const needle = deferredQ.trim().toLowerCase();
    let list = copilotList ?? apps;
    list = list.filter(
      (a) =>
        !ignorePub.some((p) => p && a.publisher?.toLowerCase() === p.toLowerCase()) &&
        !ignoreName.some((n) => n && a.name?.toLowerCase() === n.toLowerCase()),
    );
    if (category === "desktop") {
      list = list.filter((a) => a.source !== "Store");
    } else if (category === "store") {
      list = list.filter((a) => a.source === "Store");
    } else if (category === "large") {
      list = [...list]
        .filter((a) => sizeOf(a) > 200 * 1024)
        .sort((a, b) => sizeOf(b) - sizeOf(a));
    } else if (category === "recent") {
      list = [...list]
        .filter((a) => a.install_date)
        .sort((a, b) => (b.install_date || "").localeCompare(a.install_date || ""))
        .slice(0, 50);
    }
    if (needle) {
      list = list.filter(
        (a) =>
          a.name.toLowerCase().includes(needle) ||
          a.publisher.toLowerCase().includes(needle) ||
          a.install_location.toLowerCase().includes(needle),
      );
    }
    if (!sortCol) return list;
    const kbOf = (a: InstalledApp) => sizeMap?.[a.install_location] || sizeOf(a);
    const s = [...list].sort((a, b) => {
      if (sortCol === "size") {
        return kbOf(a) - kbOf(b);
      }
      if (sortCol === "recommend") {
        return recommendScore(b, kbOf(b)) - recommendScore(a, kbOf(a));
      }
      return (a.name || "").toLowerCase().localeCompare((b.name || "").toLowerCase());
    });
    return sortDesc ? s.reverse() : s;
  }, [apps, copilotList, deferredQ, sortCol, sortDesc, sizeOf, sizeMap, ignorePub, ignoreName, category]);

  const sortBy = useCallback(
    (col: "name" | "size" | "recommend") => {
      if (sortCol === col) setSortDesc((d) => !d);
      else {
        setSortCol(col);
        setSortDesc(col === "size");
      }
    },
    [sortCol, setSortCol, setSortDesc],
  );

  return { filtered, sortBy };
}
