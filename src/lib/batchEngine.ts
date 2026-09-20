/** Batch cleanup loop extracted from App (pure control flow + callbacks). */
import { api } from "./api";
import { t } from "../i18n";
import { formatError, prettyAppName } from "../lib/format";
import { defaultSelectable } from "../lib/decision";
import { toast } from "../lib/toast";
import type { InstalledApp } from "../types";
import type { BatchItemResult } from "../components/BatchPanels";

export type BatchCallbacks = {
  onIndex: (i: number) => void;
  onCurrent: (name: string) => void;
  onResults: (r: BatchItemResult[]) => void;
  onShowSummary: (v: boolean) => void;
  onSetBatching: (v: boolean) => void;
  onDoneKeys: (keys: string[]) => void;
  cancelRef: { current: boolean };
  busyRef: { current: boolean };
};

export async function runBatchCleanup(
  queue: InstalledApp[],
  useOfficial: boolean,
  appKey: (a: InstalledApp) => string,
  cb: BatchCallbacks,
  /** Opt-in backup from the batch confirm checkbox; default off. */
  backupEnabled = false,
) {
  const L = t();
  if (!queue.length) {
    toast.info(L.selectRowHint);
    return;
  }
  cb.busyRef.current = true;
  cb.cancelRef.current = false;
  cb.onSetBatching(true);
  cb.onIndex(0);
  cb.onCurrent("");
  cb.onResults([]);
  cb.onShowSummary(false);
  const results: BatchItemResult[] = [];
  const okKeys = new Set<string>();
  try {
    for (let i = 0; i < queue.length; i++) {
      if (cb.cancelRef.current) break;
      const app = queue[i];
      const key = appKey(app);
      cb.onIndex(i + 1);
      cb.onCurrent(app.name);
      toast.info(L.toastBatchProgress(i + 1, queue.length, prettyAppName(app.name, app.source)));
      try {
        const r = await api.analyze(app);
        const items = r.items.filter(defaultSelectable);
        // F-1: do not skip the whole app when there are no default-selectable leftovers.
        // Official uninstall still runs (batchUseOfficial); residual delete is a no-op.
        const report = await api.fullCleanup(app, items, {
          dry_run: false,
          skip_official_uninstall: !useOfficial,
          backup_enabled: backupEnabled,
        });
        if (report.aborted && !items.length && !useOfficial) {
          results.push({ key, name: app.name, status: "skipped", detail: "" });
          okKeys.add(key);
          continue;
        }
        const detail = L.batchDetail(report.deleted, report.failed);
        if (report.failed > 0 && report.deleted === 0) {
          results.push({ key, name: app.name, status: "failed", detail });
        } else if (!items.length && report.uninstall_ok) {
          results.push({
            key,
            name: app.name,
            status: "ok",
            detail: L.batchDetail(0, 0),
          });
          okKeys.add(key);
        } else {
          results.push({ key, name: app.name, status: "ok", detail });
          okKeys.add(key);
        }
      } catch (e) {
        results.push({
          key,
          name: app.name,
          status: "failed",
          detail: formatError(e, "cleanup"),
        });
      }
      cb.onResults([...results]);
    }
    const cancelled = cb.cancelRef.current;
    toast.info(cancelled ? L.batchCancelled : L.batchDone);
    cb.onDoneKeys([...okKeys]);
    cb.onShowSummary(true);
  } finally {
    cb.busyRef.current = false;
    cb.onSetBatching(false);
    cb.onCurrent("");
    cb.onIndex(0);
  }
}
