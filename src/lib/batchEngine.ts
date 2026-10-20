/** Batch cleanup loop extracted from App (pure control flow + callbacks). */
import { api } from "./api";
import { t } from "../i18n";
import { formatError, prettyAppName } from "../lib/format";
import { backendText } from "./backendText";
import { defaultSelectable } from "../lib/decision";
import {
  cleanupAccomplishedSomething,
  cleanupNothingText,
  cleanupOutcomeText,
} from "../lib/cleanupFeedback";
import { toast } from "../lib/toast";
import type { InstalledApp } from "../types";
import type { BatchItemResult } from "../components/BatchPanels";
import { loadRestorePointPref } from "./restorePointPref";

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
  skipOfficialKeys: ReadonlySet<string> = new Set(),
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
      if (!app) continue;
      const key = appKey(app);
      const runOfficial = useOfficial && !skipOfficialKeys.has(key);
      let officialDone = skipOfficialKeys.has(key);
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
          skip_official_uninstall: !runOfficial,
          backup_enabled: backupEnabled,
        // F-9: the user can decline a restore point; the backend still
        // owns the decision when this is absent.
        restore_point: loadRestorePointPref(),
          cleanup_source: "uninstall",
        });
        officialDone ||= runOfficial && report.uninstall_ok;
        const emptyResidual = report.aborted && !items.length && !runOfficial
          && report.uninstall_message === "no items" && !report.errors.length;
        if ((report.aborted && !emptyResidual) || (runOfficial && !report.uninstall_ok)) {
 // backup/session abort must not look like success.
          results.push({
            key,
            name: app.name,
            status: "failed",
            detail: backendText(report.uninstall_message) || L.uninstallFail,
            officialDone,
          });
          cb.onResults([...results]);
          continue;
        }
        if (emptyResidual) {
          results.push({ key, name: app.name, status: "skipped", detail: "" });
          okKeys.add(key);
          cb.onResults([...results]);
          continue;
        }
        // F-1: the per-app line states all four counts, not just deleted/failed,
        // so a run that deleted nothing is legible in the batch summary too.
        const detail = cleanupOutcomeText(report, L);
        if (report.failed > 0) {
          results.push({ key, name: app.name, status: "failed", detail, officialDone });
        } else if (!items.length && report.uninstall_ok) {
          results.push({
            key,
            name: app.name,
            status: "ok",
            detail: L.batchDetail(0, 0),
          });
          okKeys.add(key);
        } else if (!cleanupAccomplishedSomething(report)) {
          // F-1: `failed === 0` counted the app as "processed" even when the
          // keep rules refused every hand-picked row. Report it as skipped so
          // the batch summary does not claim the app was cleaned.
          results.push({
            key,
            name: app.name,
            status: "skipped",
            detail: cleanupNothingText(report, L),
            officialDone,
          });
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
          officialDone,
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
