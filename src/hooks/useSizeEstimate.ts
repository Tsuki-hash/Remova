import { useCallback, useEffect, useRef, useState } from "react";
import { formatSize } from "../lib/format";
import { api } from "../lib/api";
import { UPDATE_BUSY } from "../lib/nativeActivity";
import type { InstalledApp } from "../types";

type SizeEntry = { kb: number; capped: boolean };

/** Auto-estimate missing install sizes after list load. */
export function useSizeEstimate(apps: InstalledApp[], loading: boolean) {
  const [estimating, setEstimating] = useState(false);
  const [sizeMap, setSizeMap] = useState<Record<string, SizeEntry>>({});
  const [sizeProgress, setSizeProgress] = useState({ done: 0, total: 0 });
  const sizeCache = useRef(new Map<string, SizeEntry>());
  const activeRun = useRef<{ remaining: Set<string>; cancelled: boolean } | null>(null);
 // Native begin/cancel mutate one backend generation; keep their order even
 // when a refresh arrives while cancellation is still awaiting IPC.
  const nativeLifecycle = useRef<Promise<void>>(Promise.resolve());
 // paths abandoned by an explicit stop — never auto-restarted by
 // a later apps refresh (they can still be estimated after a remount).
  const sizeSkipped = useRef(new Set<string>());

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
 // file-cap walk is a floor — never show it as a complete size.
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
    ].filter((p) => !sizeCache.current.has(p) && !sizeSkipped.current.has(p));
    if (pending.length === 0) {
 // a re-run whose paths are all cached must not leave the footer spinner on.
      setEstimating(false);
      setSizeProgress({ done: 0, total: 0 });
      return;
    }

    let disposed = false;
    let flushTimer: ReturnType<typeof setTimeout> | null = null;
    const run = { remaining: new Set(pending), cancelled: false };
    activeRun.current = run;
    setEstimating(true);
    const total = pending.length;
    setSizeProgress({ done: 0, total });

    void (async () => {
      const begin = nativeLifecycle.current.then(async () => {
        if (!disposed && !run.cancelled) await api.beginSizeEstimate();
      }).catch(() => {});
      nativeLifecycle.current = begin;
      await begin;
      if (disposed || run.cancelled) return;
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
        while (pending.length > 0 && !disposed && !run.cancelled) {
          const path = pending.shift();
          if (!path) break;
          try {
            const est = await api.estimateDirSizeKb(path);
            if (disposed || run.cancelled) break;
            // A 0 KB result is not a stable fact (near-empty dir or a source
            // that failed silently) — leave it uncached so the next refresh
            // re-estimates instead of freezing a zero forever.
            if (est.kb > 0) {
              const entry: SizeEntry = { kb: est.kb, capped: est.capped };
              sizeCache.current.set(path, entry);
              queueSet(path, entry);
            }
          } catch (err) {
            if (disposed || run.cancelled) break;
            // An update slot grabbed between two estimates surfaces as
            // update:busy — stop without caching so a later refresh retries.
            if (err instanceof Error && err.message === UPDATE_BUSY) break;
            // Real failures are not cached either: the path stays
            // unestimated instead of freezing a kb:0 result forever.
          }
          run.remaining.delete(path);
          done += 1;
          if (!disposed) setSizeProgress({ done, total });
        }
      });
      await Promise.all(workers);
      if (flushTimer) {
        clearTimeout(flushTimer);
        flushTimer = null;
        if (!disposed) flush();
      }
      if (!disposed) {
        if (activeRun.current === run) activeRun.current = null;
        setEstimating(false);
        setSizeProgress({ done: 0, total: 0 });
      }
    })();

    return () => {
      disposed = true;
      if (activeRun.current === run) activeRun.current = null;
      if (flushTimer) {
        clearTimeout(flushTimer);
        flushTimer = null;
      }
      setEstimating(false);
    };
  }, [apps, loading]);

  const stopSizeEstimate = useCallback(async () => {
    const run = activeRun.current;
    if (!run) return;
    run.cancelled = true;
 // Capture BOTH queued and in-flight paths synchronously. Refreshes can
 // start a new run before old native calls return; cancellation is per run.
    run.remaining.forEach(path => sizeSkipped.current.add(path));
    setEstimating(false);
    setSizeProgress({ done: 0, total: 0 });
    try {
      const cancel = nativeLifecycle.current.then(() => api.cancelSizeEstimate()).then(() => {});
      nativeLifecycle.current = cancel.catch(() => {});
      await cancel;
    } catch {
 // ignore
    }
  }, []);

  return { estimating, sizeProgress, stopSizeEstimate, sizeOf, formatAppSize };
}
