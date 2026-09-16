import { useCallback, useEffect, useRef, useState } from "react";
import { formatSize } from "../i18n";
import { api } from "../lib/api";
import type { InstalledApp } from "../types";

/** Auto-estimate missing install sizes after list load (P0-2). */
export function useSizeEstimate(apps: InstalledApp[], loading: boolean) {
  const [estimating, setEstimating] = useState(false);
  const [sizeMap, setSizeMap] = useState<Record<string, number>>({});
  const [sizeProgress, setSizeProgress] = useState({ done: 0, total: 0 });
  const sizeCache = useRef(new Map<string, number>());
  const sizeCancelRef = useRef(false);

  const sizeOf = useCallback((a: InstalledApp): number => {
    if (a.estimated_size_kb > 0) return a.estimated_size_kb;
    return sizeCache.current.get(a.install_location) ?? 0;
  }, []);

  const formatAppSize = useCallback((a: InstalledApp): string => {
    if (a.estimated_size_kb > 0) return formatSize(a.estimated_size_kb);
    const est = sizeCache.current.get(a.install_location) ?? 0;
    if (est > 0) return `~${formatSize(est)}`;
    return "—";
  }, []);

  useEffect(() => {
    if (loading || apps.length === 0) return;
    const pending = [
      ...new Set(
        apps
          .filter((a) => !(a.estimated_size_kb > 0) && a.install_location?.trim())
          .map((a) => a.install_location.trim()),
      ),
    ].filter((p) => !sizeCache.current.has(p));
    if (pending.length === 0) return;

    let disposed = false;
    sizeCancelRef.current = false;
    setEstimating(true);
    const total = pending.length;
    setSizeProgress({ done: 0, total });
    void api.beginSizeEstimate().catch(() => {});

    (async () => {
      let done = 0;
      const workers = Array.from({ length: 2 }, async () => {
        while (pending.length > 0 && !disposed && !sizeCancelRef.current) {
          const path = pending.shift();
          if (!path) break;
          try {
            const kb = await api.estimateDirSizeKb(path);
            if (disposed || sizeCancelRef.current) break;
            sizeCache.current.set(path, kb > 0 ? kb : 0);
            setSizeMap((m) => ({ ...m, [path]: kb > 0 ? kb : 0 }));
          } catch {
            if (disposed || sizeCancelRef.current) break;
            sizeCache.current.set(path, 0);
            setSizeMap((m) => ({ ...m, [path]: 0 }));
          }
          done += 1;
          if (!disposed) setSizeProgress({ done, total });
        }
      });
      await Promise.all(workers);
      if (!disposed) {
        setEstimating(false);
        setSizeProgress({ done: 0, total: 0 });
      }
    })();

    return () => {
      disposed = true;
    };
  }, [apps, loading]);

  const stopSizeEstimate = async () => {
    sizeCancelRef.current = true;
    setEstimating(false);
    try {
      await api.cancelSizeEstimate();
    } catch {
      // ignore
    }
  };

  return { estimating, sizeMap, sizeProgress, stopSizeEstimate, sizeOf, formatAppSize };
}
