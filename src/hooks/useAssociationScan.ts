import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { AssociationScanProgress } from "../lib/api";
import type { InstalledApp } from "../types";

export type ScanProgressState = AssociationScanProgress & {
  status: "running" | "stopping" | "cancelled" | "failed" | "complete";
};
type Job = { id?: number; cancelled: boolean; timer?: ReturnType<typeof setTimeout>; cancelRequest?: Promise<void> };
export const scanCancelled = (error: unknown) =>
  (error instanceof Error ? error.message : String(error)) === "scan:cancelled";

/** Each run owns its native task and timer; late polling/results cannot publish. */
export function useAssociationScan() {
  const active = useRef<Job | null>(null);
  const [progress, setProgress] = useState<ScanProgressState | null>(null);
  const stop = useCallback((job: Job) => {
    clearTimeout(job.timer);
    if (job.id === undefined) { job.cancelled = true; return Promise.resolve(); }
    job.cancelRequest = (async () => {
      if (await api.cancelAssociationScan(job.id!) !== false) job.cancelled = true;
    })();
    return job.cancelRequest;
  }, []);
  useEffect(() => () => {
    const job = active.current;
    active.current = null;
    if (job) void stop(job).catch(() => {});
  }, [stop]);

  const cancel = useCallback(async () => {
    const job = active.current;
    if (!job || job.cancelled) return;
    setProgress(p => p && { ...p, status: "stopping" });
    await stop(job);
    if (!job.cancelled && active.current === job) setProgress(p => p && { ...p, status: "running" });
  }, [stop]);

  const runReadOnly = useCallback(async <T,>(work: (id: number) => Promise<T>, count: (result: T) => number) => {
    if (active.current) void stop(active.current).catch(() => {});
    const job: Job = { cancelled: false };
    active.current = job;
    setProgress({ status: "running", stage: "preparing", found: 0 });
    try {
      job.id = await api.beginAssociationScan();
      if (job.cancelled || active.current !== job) throw new Error("scan:cancelled");
      const poll = async () => {
        try {
          const p = await api.associationScanProgress(job.id!);
          if (p && active.current === job && !job.cancelled && !job.cancelRequest) setProgress({ ...p, status: "running" });
        } catch { /* Progress is advisory; the scan result carries execution errors. */ }
        if (active.current === job && !job.cancelled) job.timer = setTimeout(() => void poll(), 250);
      };
      void poll();
      const result = await work(job.id);
      await job.cancelRequest?.catch(() => {});
      if (job.cancelled || active.current !== job) throw new Error("scan:cancelled");
      setProgress({ status: "complete", stage: "finalizing", found: count(result) });
      return result;
    } catch (error) {
      const cancelled = job.cancelled || active.current !== job || scanCancelled(error);
      if (active.current === job) {
        setProgress(p => p && { ...p, status: cancelled ? "cancelled" : "failed" });
      }
      if (cancelled) throw new Error("scan:cancelled", { cause: error });
      throw error;
    } finally {
      clearTimeout(job.timer);
      if (active.current === job) active.current = null;
      // Also release a reservation when invocation fails before a worker claims it.
      if (job.id !== undefined) void api.cancelAssociationScan(job.id).catch(() => {});
    }
  }, [stop]);
  const run = useCallback((app: InstalledApp) =>
    runReadOnly(id => api.analyze(app, id), result => result.items.length), [runReadOnly]);
  const runItems = useCallback(<T,>(work: (id: number) => Promise<T[]>) =>
    runReadOnly(work, result => result.length), [runReadOnly]);
  return { run, runItems, runReadOnly, cancel, progress };
}
