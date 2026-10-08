import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { AssociationScanProgress } from "../lib/api";
import type { InstalledApp } from "../types";

export type ScanProgressState = AssociationScanProgress & {
  status: "running" | "stopping" | "cancelled" | "failed" | "complete";
};
type Job = { id?: number; cancelled: boolean; timer?: ReturnType<typeof setTimeout> };
export const scanCancelled = (error: unknown) =>
  (error instanceof Error ? error.message : String(error)) === "scan:cancelled";

/** Each run owns its native task and timer; late polling/results cannot publish. */
export function useAssociationScan() {
  const active = useRef<Job | null>(null);
  const [progress, setProgress] = useState<ScanProgressState | null>(null);
  const stop = useCallback(async (job: Job) => {
    job.cancelled = true;
    clearTimeout(job.timer);
    if (job.id !== undefined) await api.cancelAssociationScan(job.id);
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
  }, [stop]);

  const run = useCallback(async (app: InstalledApp) => {
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
          if (p && active.current === job && !job.cancelled) setProgress({ ...p, status: "running" });
        } catch { /* Progress is advisory; the scan result carries execution errors. */ }
        if (active.current === job && !job.cancelled) job.timer = setTimeout(() => void poll(), 250);
      };
      void poll();
      const result = await api.analyze(app, job.id);
      if (job.cancelled || active.current !== job) throw new Error("scan:cancelled");
      setProgress({ status: "complete", stage: "finalizing", found: result.items.length });
      return result;
    } catch (error) {
      if (active.current === job) {
        const cancelled = job.cancelled || scanCancelled(error);
        setProgress(p => p && { ...p, status: cancelled ? "cancelled" : "failed" });
        if (cancelled) throw new Error("scan:cancelled", { cause: error });
      }
      throw error;
    } finally {
      clearTimeout(job.timer);
      if (active.current === job) active.current = null;
      // Also release a reservation when invocation fails before a worker claims it.
      if (job.id !== undefined) void api.cancelAssociationScan(job.id).catch(() => {});
    }
  }, [stop]);
  return { run, cancel, progress };
}
