import { useCallback, useRef, useState } from "react";
import { api } from "../lib/api";
import type { CleanupReport, FullCleanupReport, InstalledApp, ScanResult } from "../types";
import type { Strings } from "../i18n";
import { formatError, prettyAppName } from "../lib/format";
import { backendText } from "../lib/backendText";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import {
  buildCleanupRiskBits,
  defaultSelectable,
  formatRiskNote,
  maxRiskOf,
  riskTierLabel,
} from "../lib/decision";
import { appKey, scanMatchesApp } from "../lib/appKey";
import { runBatchCleanup } from "../lib/batchEngine";
import { cleanupAccomplishedSomething, cleanupFeedback, cleanupNothingText, cleanupOutcomeText } from "../lib/cleanupFeedback";
import type { BatchItemResult } from "../components/BatchPanels";
import { loadRestorePointPref } from "../lib/restorePointPref";

/** Grouped cleanup setters. */
export type CleanupFlowSetters = {
  setSelectedPaths?: (paths: Set<string>) => void;
  setScan?: (scan: ScanResult | null) => void;
  setMulti: (updater: Set<string> | ((m: Set<string>) => Set<string>)) => void;
  setResidualFromUninstall: (v: boolean) => void;
  setAiRisk: (v: string | null) => void;
  setReport: (r: CleanupReport | FullCleanupReport | null) => void;
  setVerifyRows: (rows: { path: string; kind: string; still_there: boolean }[] | null) => void;
  setAiReportNote: (v: string | null) => void;
  setError: (e: string | null) => void;
};

/** Force-clean / dry-run / real cleanup / beginner batch handlers. */
export function useCleanupHandlers({
  L,
  selected: listSelected,
  scanTarget = listSelected,
  currentReport,
  scan,
  selectedPaths,
  residualFromUninstall,
  useOfficial,
  aiEnabled,
  aiRisk,
  apps,
  multi,
  flow,
  refreshApps,
  onAfterCleanup,
  busyRef,
}: {
  L: Strings;
  selected: InstalledApp | null;
  scanTarget?: InstalledApp | null;
  currentReport?: { current: CleanupReport | FullCleanupReport | null };
  scan: ScanResult | null;
  selectedPaths: Set<string>;
  residualFromUninstall: boolean;
  useOfficial: boolean;
  aiEnabled: boolean;
  aiRisk?: string | null;
  apps: InstalledApp[];
  multi: Set<string>;
  flow: CleanupFlowSetters;
  refreshApps: () => Promise<void>;
  /** auto re-analyze after successful official uninstall + cleanup. */
  onAfterCleanup?: (app: InstalledApp, report: FullCleanupReport) => void;
  busyRef: { current: boolean };
}) {
  const selected = scan && scanTarget && scanMatchesApp(scan, scanTarget) ? scanTarget : null;
  const runOfficial = !!selected && useOfficial && !residualFromUninstall &&
    selected.source !== "Monitor" && selected.source !== "Orphan";
  const {
    setMulti,
    setSelectedPaths,
    setScan,
    setResidualFromUninstall,
    setAiRisk,
    setReport,
    setVerifyRows,
    setAiReportNote,
    setError,
  } = flow;
  const [forceBusy, setForceBusy] = useState(false);
  const [dryRunning, setDryRunning] = useState(false);
  /** Beginner batch path runs official uninstaller by default. */
  const batchUseOfficial = true;
  const [batching, setBatching] = useState(false);
  const [batchIndex, setBatchIndex] = useState(0);
  const [batchTotal, setBatchTotal] = useState(0);
  const [batchCurrent, setBatchCurrent] = useState("");
  const [batchResults, setBatchResults] = useState<BatchItemResult[]>([]);
  const [showBatchSummary, setShowBatchSummary] = useState(false);
  const batchCancelRef = useRef(false);
  const batchQueueRef = useRef<InstalledApp[]>([]);

  const backupEnabledRef = useRef(false);
  /** Invalidates an in-flight verify probe when a newer cleanup starts. */
  const verifySeqRef = useRef(0);
  /** Only the latest dry-run may write its report. */
  const dryRunSeqRef = useRef(0);
  /** Invalidates force-clean's internal analyze when a newer one starts. */
  const forceAnalyzeSeqRef = useRef(0);

  const forceClean = useCallback(
    async (appOverride?: InstalledApp) => {
      const target = appOverride ?? listSelected;
      if (!target || forceBusy || busyRef.current) return;
      setForceBusy(true);
      busyRef.current = true;
      const aseq = ++forceAnalyzeSeqRef.current;
      let scanId: number | undefined;
      try {
        // Use a real scan task: the registry caches are only reused inside one,
        // and the task is what makes the walk cancellable and reportable.
        scanId = await api.beginAssociationScan();
        const r = await api.analyze(target, scanId);
 // A newer force-clean/analyze started while this one was in flight.
        if (aseq !== forceAnalyzeSeqRef.current) return;
        const items = r.items.filter(defaultSelectable);
        if (!items.length) {
          toast.info(L.toastForceCleanEmpty);
          return;
        }
        const forceRiskNote = formatRiskNote(buildCleanupRiskBits(items, L), L.riskNoteTitle);
        const { ok, checked } = await requestConfirmEx({
          title: L.forceClean,
          message: `${prettyAppName(target.name, target.source)}\n${L.confirmForceRiskPrefix(riskTierLabel(maxRiskOf(items), L))}${forceRiskNote}\n${L.forceCleanHint}`,
          confirmLabel: L.forceClean,
          danger: true,
          checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false, status: L.cleanupBackupStatus },
        });
        if (!ok) return;
        const report = await api.fullCleanup(target, items, {
          dry_run: false,
          skip_official_uninstall: true,
          backup_enabled: checked,
        // the user can decline a restore point; the backend still
        // owns the decision when this is absent.
        restore_point: loadRestorePointPref(),
          cleanup_source:
            target.source === "Orphan" || target.source === "Monitor"
              ? (target.source.toLowerCase() as "orphan" | "monitor")
              : "uninstall",
        });
        if (report.aborted) {
          toast.error(backendText(report.uninstall_message) || L.uninstallFail);
          setError(backendText(report.uninstall_message) || L.uninstallFail);
          setReport(report);
          setVerifyRows(null);
          setAiReportNote(null);
          return;
        }
        cleanupFeedback(report, L, undefined, `${L.forceClean}: ${prettyAppName(target.name, target.source)} · `);
        setReport(report);
 // a new report must never inherit the previous cleanup's
 // post-cleanup verify list or AI narrative.
        setVerifyRows(null);
        setAiReportNote(null);
        void refreshApps();
      } catch (e) {
        if (scanId !== undefined) void api.cancelAssociationScan(scanId).catch(() => {});
        setError(formatError(e, "cleanup"));
        toast.error(formatError(e, "cleanup"));
      } finally {
        busyRef.current = false;
        setForceBusy(false);
      }
    },
    [listSelected, forceBusy, L, refreshApps, setError, setReport, busyRef, setVerifyRows, setAiReportNote],
  );

  const dryRun = useCallback(async () => {
    if (!scan || !selected) return;
 // Never overlap dry-run with another busy cleanup/uninstall.
    if (busyRef.current) return;
    busyRef.current = true;
    const items = scan.items.filter((it) => selectedPaths.has(it.path));
    const seq = ++dryRunSeqRef.current;
    setDryRunning(true);
    try {
      const r = await api.dryRun(selected, items, {
        cleanup_source:
          selected.source === "Monitor"
            ? "monitor"
            : selected.source === "Orphan"
              ? "orphan"
              : "uninstall",
      });
      if (seq !== dryRunSeqRef.current) return;
      setReport(r);
      setVerifyRows(null);
      setAiReportNote(null);
    } catch (e) {
      if (seq !== dryRunSeqRef.current) return;
      setError(formatError(e, "cleanup"));
    } finally {
      if (seq === dryRunSeqRef.current) setDryRunning(false);
      busyRef.current = false;
    }
  }, [
    scan,
    selected,
    selectedPaths,
    setReport,
    setVerifyRows,
    setAiReportNote,
    setError,
    busyRef,
  ]);

  const execReal = useCallback(async (opts?: { slotHeld?: boolean }) => {
    if (!scan || !selected) {
      if (opts?.slotHeld) busyRef.current = false;
      return;
    }
    if (opts?.slotHeld) {
 // Caller already owns busyRef (confirm pipeline).
    } else if (busyRef.current) {
      return;
    } else {
      busyRef.current = true;
    }
    const items = scan.items.filter((it) => selectedPaths.has(it.path));
    verifySeqRef.current += 1;
    setDryRunning(true);
    let afterCleanup: { app: InstalledApp; report: FullCleanupReport } | null = null;
    try {
      const r = await api.fullCleanup(selected, items, {
        dry_run: false,
 // Residual cleanup after official uninstall never re-runs official uninstaller.
 // Normal analysis runs official uninstall unless advanced direct cleanup is selected.
        skip_official_uninstall: !runOfficial,
        backup_enabled: backupEnabledRef.current,
        // the user can decline a restore point; the backend still
        // owns the decision when this is absent.
        restore_point: loadRestorePointPref(),
        cleanup_source:
          selected.source === "Monitor"
            ? "monitor"
            : selected.source === "Orphan"
              ? "orphan"
              : "uninstall",
      });
      setReport(r);
      if (r.aborted) {
        const msg = backendText(r.uninstall_message) || L.uninstallFail;
        // A 1641/pending report is not a failure: the uninstaller succeeded and
        // deletion was postponed until the restart. Do not style it as one.
        if (r.reboot_pending) toast.info(msg);
        else toast.error(msg);
        setError(msg);
        setVerifyRows(null);
        setAiReportNote(null);
        // A 1641 abort still means the official uninstaller succeeded. Keep the
        // post-uninstall state so a retry never re-runs the uninstaller.
        setResidualFromUninstall(r.uninstall_ok || residualFromUninstall);
        void refreshApps();
        return;
      }
      setAiReportNote(null);
      setVerifyRows(null);
      setSelectedPaths?.(new Set());
      setScan?.(null);
      if (r.failed > 0) {
        toast.error(L.batchDetail(r.deleted, r.failed));
      } else if (!cleanupAccomplishedSomething(r)) {
        // `failed === 0` here used to mean "success". A selection the keep
        // rules refused every row of also has `failed === 0`, and it was
        // reported as a green "成功 0 · 失败 0".
        toast.error(cleanupNothingText(r, L));
      } else {
        toast.success(cleanupOutcomeText(r, L));
      }
 // SOP checklist: re-probe cleaned paths
      const vseq = ++verifySeqRef.current;
      void api
        .verifyLeftovers(items)
        .then((rows) => {
          if (vseq === verifySeqRef.current && (!currentReport || currentReport.current === r)) setVerifyRows(rows);
        })
        .catch(() => {});
      setResidualFromUninstall(r.uninstall_ok || residualFromUninstall);
      if (r.uninstall_ok && selected) {
        afterCleanup = { app: selected, report: r };
      }
      void refreshApps();
    } catch (e) {
      setError(formatError(e, "cleanup"));
      toast.error(formatError(e, "cleanup"));
    } finally {
      backupEnabledRef.current = false;
      busyRef.current = false;
      setDryRunning(false);
    }
    // Start the post-cleanup rescan only after the cleanup released busyRef;
    // otherwise the nested analyze inherits an external hold while the ref is
    // already free, letting another destructive action start mid-scan.
    if (afterCleanup) onAfterCleanup?.(afterCleanup.app, afterCleanup.report);
  }, [
    scan,
    selected,
    selectedPaths,
    refreshApps,
    runOfficial,
    setSelectedPaths,
    setScan,
    residualFromUninstall,
    L,
    onAfterCleanup,
    setReport,
    setAiReportNote,
    setVerifyRows,
    setResidualFromUninstall,
    setError,
    busyRef,
    currentReport,
  ]);

  /** Confirm vault + compressed key risks (full narrative lives in scan conclusion). */
  const handleCleanupConfirm = useCallback(async () => {
    if (!scan || !selected) return;
    if (busyRef.current) {
      toast.info(L.taskBusy);
      return;
    }
 // Hold busyRef across AI brief + confirm so execReal cannot silently no-op.
    busyRef.current = true;
    try {
    const picked = scan.items.filter((it) => selectedPaths.has(it.path));
    const n = picked.length;
    if (n === 0) {
 // FE-N3 / never confirm an empty cleanup set — say "select rows", not "cleanup".
      toast.info(L.selectRowHint);
      busyRef.current = false;
      return;
    }
    let message = `${L.riskTierPrefix(riskTierLabel(maxRiskOf(picked), L))}\n${L.cleanupConfirmOptionalBackup(
      n,
      runOfficial,
    )}`;
 // Never truncate: high-risk must stay visible.
    message = `${message}${formatRiskNote(buildCleanupRiskBits(picked, L), L.riskNoteTitle)}`;
    if (picked.some((it) => /\\common files\\/i.test(it.path))) {
      message = `${message}\n\n${L.confirmCommonFilesHint}`;
    }
    if (aiEnabled && selected && aiRisk) {
      message = `${message}\n\n${L.aiRiskTitle}: ${aiRisk.slice(0, 160)}`;
    } else if (aiEnabled && selected) {
      try {
        const kinds: Record<string, number> = {};
        for (const it of picked) {
          kinds[it.kind] = (kinds[it.kind] || 0) + 1;
        }
        const brief = await api.aiRiskBrief({
          appName: selected.name,
          publisher: selected.publisher || "",
          action: residualFromUninstall ? "residual_cleanup" : "cleanup",
          itemCount: n,
          kindCounts: kinds,
          hasService: picked.some((it) => it.kind.toLowerCase().includes("service")),
          hasRunKey: picked.some(
            (it) =>
              it.path.toLowerCase().includes("\\run") || it.kind.toLowerCase().includes("run"),
          ),
          hasSharedHint: picked.some((it) => it.shared),
        });
        if (brief) {
          setAiRisk(brief);
          message = `${message}\n\n${L.aiRiskTitle}: ${brief.slice(0, 160)}`;
        } else {
          // `ai_risk_brief` returns Ok(None) when the feature is off, so
          // the confirm dialog silently lacked the AI line. Say it is absent.
          setAiRisk(null);
          message = `${message}\n\n${L.aiRiskBriefFailed}`;
        }
      } catch (e) {
 // the AI brief used to fail silently, so a missing model line read as
 // "AI already assessed this". Drop any stale brief and say the brief is absent.
        setAiRisk(null);
        message = `${message}\n\n${L.aiRiskBriefFailed}`;
        toast.error(formatError(e, "analyze"));
      }
    }
    const { ok, checked } = await requestConfirmEx({
      title: L.cleanup,
      message,
      confirmLabel: L.cleanup,
      danger: true,
      checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false, status: L.cleanupBackupStatus },
    });
    if (!ok) {
      busyRef.current = false;
      return;
    }
    backupEnabledRef.current = checked;
    await execReal({ slotHeld: true });
    } catch {
      busyRef.current = false;
    }
  }, [
    scan,
    selected,
    selectedPaths,
    residualFromUninstall,
    runOfficial,
    aiEnabled,
    aiRisk,
    L,
    setAiRisk,
    execReal,
    busyRef,
  ]);

  const batchCleanup = useCallback(
    async (queueOverride?: InstalledApp[], skipOfficialKeys: ReadonlySet<string> = new Set()) => {
 // one Set for the whole filter, not per row.
      const multiSet = new Set(multi);
      const queue = queueOverride ?? apps.filter((a) => multiSet.has(appKey(a)));
      if (!queue.length) {
        toast.info(L.selectRowHint);
        return;
      }
      if (busyRef.current || batching) return;
      const { ok, checked } = await requestConfirmEx({
        title: L.batchUninstall,
        message: L.batchConfirm(queue.length, queue.some(a => !skipOfficialKeys.has(appKey(a)))),
        confirmLabel: L.batchUninstall,
        danger: true,
        checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false, status: L.cleanupBackupStatus },
      });
      if (!ok) return;
 // confirm is async — another cleanup may have taken busyRef meanwhile.
      if (busyRef.current || batching) return;
      setBatchTotal(queue.length);
      batchQueueRef.current = queue;
      try {
        busyRef.current = true;
        await runBatchCleanup(
          queue,
          batchUseOfficial,
          appKey,
          {
            onIndex: setBatchIndex,
            onCurrent: setBatchCurrent,
            onResults: setBatchResults,
            onShowSummary: setShowBatchSummary,
            onSetBatching: setBatching,
            onDoneKeys: (keys) => {
 // remove finished keys without clearing failed multi selections.
              setMulti((m) => {
                const okSet = new Set(keys);
                const n = new Set(m);
                for (const k of okSet) n.delete(k);
                return n;
              });
            },
            cancelRef: batchCancelRef,
            busyRef,
          },
          checked,
          skipOfficialKeys,
        );
 // List must reflect uninstalled apps immediately.
        await refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
      }
    },
    [apps, multi, L, setMulti, setError, busyRef, batchUseOfficial, batching, refreshApps],
  );

  const cancelBatch = useCallback(() => {
    batchCancelRef.current = true;
    void api.cancelCleanup().catch(() => {});
    toast.info(L.batchCancelHint);
  }, [L]);

  const cancelCleanup = useCallback(() => {
    void api.cancelCleanup().catch(() => {});
  }, []);

  const retryFailedBatch = useCallback(() => {
    const failed = batchResults.filter((r) => r.status === "failed");
    if (!failed.length) return;
    const keys = new Set(failed.map(r => r.key));
    const queue = batchQueueRef.current.filter(a => keys.has(appKey(a)));
    const officialDone = new Set(failed.filter(r => r.officialDone).map(r => r.key));
    void batchCleanup(queue, officialDone).catch(e => setError(formatError(e, "cleanup")));
  }, [batchResults, batchCleanup, setError]);

  return {
    forceBusy,
    dryRunning,
    batching,
    batchIndex,
    batchTotal,
    batchCurrent,
    batchResults,
    showBatchSummary,
    setShowBatchSummary,
    forceClean,
    dryRun,
    handleCleanupConfirm,
    batchCleanup,
    cancelBatch,
    cancelCleanup,
    retryFailedBatch,
  };
}
