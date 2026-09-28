import { useCallback, useEffect, useRef, useState } from "react";
import { formatSize } from "../lib/format";
import { api } from "../lib/api";
import type { InstalledApp } from "../types";

type SizeEntry = { kb: number; capped: boolean };

/** Auto-estimate missing install sizes after list load (P0-2). */
export function useSizeEstimate(apps: InstalledApp[], loading: boolean) {
  const [estimating, setEstimating] = useState(false);
  const [sizeMap, setSizeMap] = useState<Record<string, SizeEntry>>({});
  const [sizeProgress, setSizeProgress] = useState({ done: 0, total: 0 });
  const sizeCache = useRef(new Map<string, SizeEntry>());
  const sizeCancelRef = useRef(false);

  const sizeOf = useCallback(
    (a: InstalledApp): number => {
      if (a.estimated_size_kb > 0) return a.estimated_size_kb;
      return (
        sizeMap[a.install_location]?.kb ??
        sizeCache.current.get(a.install_location)?.kb ??
        0
      );
    },
    [sizeMap],
  );

  const formatAppSize = useCallback(
    (a: InstalledApp): string => {
      if (a.estimated_size_kb > 0) return formatSize(a.estimated_size_kb);
      const est =
        sizeMap[a.install_location] ??
        sizeCache.current.get(a.install_location) ??
        null;
      if (est && est.kb > 0) {
        // R21-SUP-04: file-cap walk is a floor — never show it as a complete size.
        return est.capped ? `>=${formatSize(est.kb)}` : `~${formatSize(est.kb)}`;
      }
      return "—";
    },
    [sizeMap],
  );

  useEffect(() => {
    if (loading || apps.length === 0) return;
    const pending = [
      ...new Set(
        apps
          .filter((a) => !(a.estimated_size_kb > 0) && a.install_location?.trim())
          .map((a) => a.install_location.trim()),
      ),
    ].filter((p) => !sizeCache.current.has(p));
    if (pending.length === 0) {
      // a re-run whose paths are all cached must not leave the footer spinner on.
      setEstimating(false);
      setSizeProgress({ done: 0, total: 0 });
      return;
    }

    let disposed = false;
    let flushTimer: ReturnType<typeof setTimeout> | null = null;
    sizeCancelRef.current = false;
    setEstimating(true);
    const total = pending.length;
    setSizeProgress({ done: 0, total });
    void api.beginSizeEstimate().catch(() => {});

    void (async () => {
      let done = 0;
      let pendingFlush: Record<string, SizeEntry> = {};
      const flush = () => {
        const batch = pendingFlush;
        pendingFlush = {};
        flushTimer = null;
        if (Object.keys(batch).length === 0) return;
        setSizeMap((m) => ({ ...m, ...batch }));
      };
      const queueSet = (path: string, entry: SizeEntry) => {
        pendingFlush[path] = entry;
        if (!flushTimer) flushTimer = setTimeout(flush, 80);
      };
      const workers = Array.from({ length: 2 }, async () => {
        while (pending.length > 0 && !disposed && !sizeCancelRef.current) {
          const path = pending.shift();
          if (!path) break;
          try {
            const est = await api.estimateDirSizeKb(path);
            if (disposed || sizeCancelRef.current) break;
            const entry: SizeEntry = {
              kb: est.kb > 0 ? est.kb : 0,
              capped: est.capped,
            };
            sizeCache.current.set(path, entry);
            queueSet(path, entry);
          } catch {
            if (disposed || sizeCancelRef.current) break;
            const entry: SizeEntry = { kb: 0, capped: false };
            sizeCache.current.set(path, entry);
            queueSet(path, entry);
          }
          done += 1;
          if (!disposed) setSizeProgress({ done, total });
        }
      });
      await Promise.all(workers);
      if (flushTimer) {
        clearTimeout(flushTimer);
        flushTimer = null;
        flush();
      }
      if (!disposed) {
        setEstimating(false);
        setSizeProgress({ done: 0, total: 0 });
      }
    })();

    return () => {
      disposed = true;
      if (flushTimer) {
        clearTimeout(flushTimer);
        flushTimer = null;
      }
      setEstimating(false);
    };
  }, [apps, loading]);

  const stopSizeEstimate = useCallback(async () => {
    sizeCancelRef.current = true;
    setEstimating(false);
    try {
      await api.cancelSizeEstimate();
    } catch {
      // ignore
    }
  }, []);

  return { estimating, sizeProgress, stopSizeEstimate, sizeOf, formatAppSize };
}
