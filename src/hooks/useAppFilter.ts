import { useCallback, useMemo } from "react";
import type { InstalledApp } from "../types";
import { appKey } from "../lib/appKey";
import { recommendScore, LARGE_APP_KB } from "../lib/decision";
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
  setSortCol: (c: SortCol) => void;
  setSortDesc: (updater: boolean | ((d: boolean) => boolean)) => void;
}) {
  const filtered = useMemo(() => {
    const needle = deferredQ.trim().toLowerCase();
    // performance fix: this Map is only read in the `copilotList !== null` branch, yet it
    // was rebuilt (300 `appKey` hashes) on every recompute — including every
    // keystroke and every size-estimate batch, when the branch is unreachable.
    let list = copilotList === null ? apps : (() => {
      const current = new Map(apps.map(app => [appKey(app), app]));
      return copilotList.flatMap(app => {
        const latest = current.get(appKey(app));
        return latest ? [latest] : [];
      });
    })();
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
        .filter((a) => sizeOf(a) > LARGE_APP_KB)
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
          (a.name || "").toLowerCase().includes(needle) ||
          (a.publisher || "").toLowerCase().includes(needle) ||
          (a.install_location || "").toLowerCase().includes(needle),
      );
    }
    if (!sortCol) return list;
    // performance fix: decorate-sort-undecorate. The comparator used to call
    // `recommendScore` (regex + `Date` parsing) inside every comparison —
    // O(n log n) evaluations, about 5,000 regexes for a 300-row list. Each row is
    // scored exactly once here instead.
    const decorated = [...list].map((app) => ({
      app,
      name: (app.name || "").toLowerCase(),
      kb: sortCol === "size" ? sizeOf(app) : 0,
      // `recommendScore` is only meaningful for the recommend column; higher
      // score sorts first, which the descending comparator below reproduces.
      score: sortCol === "recommend" ? recommendScore(app, sizeOf(app)) : 0,
    }));
    decorated.sort((a, b) =>
      sortCol === "name"
        ? a.name.localeCompare(b.name)
        : sortCol === "recommend"
          ? b.score - a.score
          : a.kb - b.kb,
    );
    const s = decorated.map((d) => d.app);
    return sortDesc ? s.reverse() : s;
  }, [apps, copilotList, deferredQ, sortCol, sortDesc, sizeOf, ignorePub, ignoreName, category]);

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
