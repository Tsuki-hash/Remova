import { useCallback, useRef, useState } from "react";
import { api } from "../lib/api";
import type { CleanupReport, FullCleanupReport, InstalledApp, ScanResult } from "../types";
import type { Strings } from "../i18n";
import { formatError, prettyAppName } from "../lib/format";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { defaultSelectable, maxRiskOf, riskTierLabel } from "../lib/decision";
import { appKey } from "../lib/appKey";
import { runBatchCleanup } from "../lib/batchEngine";
import type { BatchItemResult } from "../components/BatchPanels";

/** Grouped cleanup setters (A-4). */
export type CleanupFlowSetters = {
  setMulti: (updater: Set<string> | ((m: Set<string>) => Set<string>)) => void;
  setResidualFromUninstall: (v: boolean) => void;
  setAiRisk: (v: string | null) => void;
  setReport: (r: CleanupReport | FullCleanupReport | null) => void;
  setLastReport: (r: FullCleanupReport | null) => void;
  setVerifyRows: (rows: { path: string; kind: string; still_there: boolean }[] | null) => void;
  setAiReportNote: (v: string | null) => void;
  setError: (e: string | null) => void;
};

/** Force-clean / dry-run / real cleanup / beginner batch handlers. */
export function useCleanupHandlers({
  L,
  selected,
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
  /** FN-04: auto re-analyze after successful official uninstall + cleanup. */
  onAfterCleanup?: (app: InstalledApp, report: FullCleanupReport) => void;
  busyRef: { current: boolean };
}) {
  const {
    setMulti,
    setResidualFromUninstall,
    setAiRisk,
    setReport,
    setLastReport,
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

  const backupEnabledRef = useRef(false);

  const forceClean = useCallback(
    async (appOverride?: InstalledApp) => {
      const target = appOverride ?? selected;
      if (!target || forceBusy) return;
      setForceBusy(true);
      busyRef.current = true;
      try {
        const r = await api.analyze(target);
        const items = r.items.filter(defaultSelectable);
        if (!items.length) {
          toast.info(L.toastForceCleanEmpty);
          return;
        }
        const { ok, checked } = await requestConfirmEx({
          title: L.forceClean,
          message: `${prettyAppName(target.name, target.source)}\n${L.confirmForceRiskPrefix(riskTierLabel(maxRiskOf(items), L))}\n${L.forceCleanHint}`,
          confirmLabel: L.forceClean,
          danger: true,
          checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false },
        });
        if (!ok) return;
        const report = await api.fullCleanup(target, items, {
          dry_run: false,
          skip_official_uninstall: true,
          backup_enabled: checked,
          cleanup_source:
            target.source === "Orphan" || target.source === "Monitor"
              ? (target.source.toLowerCase() as "orphan" | "monitor")
              : "uninstall",
        });
        if (report.aborted) {
          toast.error(report.uninstall_message || L.errCleanupFailed("aborted"));
          setError(report.uninstall_message || L.errCleanupFailed("aborted"));
          setLastReport(report);
          setVerifyRows(null);
          setAiReportNote(null);
          return;
        }
        toast.success(
          `${L.forceClean}: ${prettyAppName(target.name, target.source)} · ${L.batchDetail(report.deleted, report.failed)}`,
        );
        setLastReport(report);
        void refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
        toast.error(L.errCleanupFailed(formatError(e, "cleanup")));
      } finally {
        busyRef.current = false;
        setForceBusy(false);
      }
    },
    [selected, forceBusy, L, refreshApps, setError, setLastReport, busyRef, setVerifyRows, setAiReportNote],
  );

  const dryRun = useCallback(async () => {
    if (!scan || !selected) return;
    const items = scan.items.filter((it) => selectedPaths.has(it.path));
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
      setReport(r);
    } catch (e) {
      setError(formatError(e, "cleanup"));
    } finally {
      setDryRunning(false);
    }
  }, [scan, selected, selectedPaths, setReport, setError]);

  const execReal = useCallback(async () => {
    if (!scan || !selected) return;
    if (busyRef.current) return;
    const items = scan.items.filter((it) => selectedPaths.has(it.path));
    busyRef.current = true;
    setDryRunning(true);
    try {
      const r = await api.fullCleanup(selected, items, {
        dry_run: false,
        // Residual cleanup after official uninstall never re-runs official uninstaller.
        // Deep-analyze path may still opt in via the checkbox.
        skip_official_uninstall:
          residualFromUninstall ||
          !useOfficial ||
          selected.source === "Monitor" ||
          selected.source === "Orphan",
        backup_enabled: backupEnabledRef.current,
        cleanup_source:
          selected.source === "Monitor"
            ? "monitor"
            : selected.source === "Orphan"
              ? "orphan"
              : "uninstall",
      });
      setReport(r);
      if (r && typeof r === "object" && "aborted" in r && r.aborted) {
        const fr = r as FullCleanupReport;
        const msg = fr.uninstall_message || L.errCleanupFailed("aborted");
        toast.error(msg);
        setError(msg);
        setLastReport(fr);
        setVerifyRows(null);
        setAiReportNote(null);
        setResidualFromUninstall(false);
        void refreshApps();
        return;
      }
      if (r && typeof r === "object" && "deleted" in r) {
        setLastReport(r as FullCleanupReport);
        setAiReportNote(null);
        setVerifyRows(null);
        const fr = r as FullCleanupReport;
        if (fr.failed > 0) {
          toast.error(L.batchDetail(fr.deleted, fr.failed));
        } else {
          toast.success(L.batchDetail(fr.deleted, fr.failed));
        }
        // SOP checklist: re-probe cleaned paths
        void api
          .verifyLeftovers(items)
          .then((rows) => setVerifyRows(rows))
          .catch(() => {});
        if (fr.uninstall_ok && !fr.aborted && selected) {
          onAfterCleanup?.(selected, fr);
        }
      }
      setResidualFromUninstall(false);
      void refreshApps();
    } catch (e) {
      setError(formatError(e, "cleanup"));
      toast.error(L.errCleanupFailed(formatError(e, "cleanup")));
    } finally {
      backupEnabledRef.current = false;
      busyRef.current = false;
      setDryRunning(false);
    }
  }, [
    scan,
    selected,
    selectedPaths,
    refreshApps,
    residualFromUninstall,
    useOfficial,
    L,
    onAfterCleanup,
    setReport,
    setLastReport,
    setAiReportNote,
    setVerifyRows,
    setResidualFromUninstall,
    setError,
    busyRef,
  ]);

  /** Confirm vault + compressed key risks (full narrative lives in scan conclusion). */
  const handleCleanupConfirm = useCallback(async () => {
    if (!scan) return;
    if (busyRef.current) return;
    const n = selectedPaths.size;
    if (n === 0) {
      // FE-N3: never confirm an empty cleanup set.
      toast.info(L.cleanup);
      return;
    }
    const picked = scan.items.filter((it) => selectedPaths.has(it.path));
    let message = `${L.riskTierPrefix(riskTierLabel(maxRiskOf(picked), L))}\n${L.cleanupConfirmOptionalBackup(
      n,
      residualFromUninstall || useOfficial,
    )}`;
    const riskBits: string[] = [];
    if (picked.some((it) => it.shared)) riskBits.push(L.confirmSharedSelected);
    if (picked.some((it) => it.user_data)) riskBits.push(L.conclusionUserDataHint);
    if (picked.some((it) => it.risk === "high")) riskBits.push(L.conclusionHighRiskHint);
    if (riskBits.length) {
      message = `${message}\n\n⚠ ${riskBits.slice(0, 2).join("\n")}`;
    }
    if (picked.some((it) => /\\common files\\/i.test(it.path))) {
      message = `${message}\n\n⚠ ${L.confirmCommonFilesHint}`;
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
        }
      } catch {
        // non-blocking
      }
    }
    const { ok, checked } = await requestConfirmEx({
      title: L.cleanup,
      message,
      confirmLabel: L.cleanup,
      danger: true,
      checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false },
    });
    if (!ok) return;
    backupEnabledRef.current = checked;
    void execReal();
  }, [
    scan,
    selected,
    selectedPaths,
    residualFromUninstall,
    useOfficial,
    aiEnabled,
    aiRisk,
    L,
    setAiRisk,
    execReal,
    busyRef,
  ]);

  const batchCleanup = useCallback(
    async (queueOverride?: InstalledApp[]) => {
      const queue = queueOverride ?? apps.filter((a) => new Set(multi).has(appKey(a)));
      if (!queue.length) {
        toast.info(L.selectRowHint);
        return;
      }
      if (busyRef.current || batching) return;
      const { ok, checked } = await requestConfirmEx({
        title: L.batchUninstall,
        message: L.batchConfirm(queue.length, batchUseOfficial),
        confirmLabel: L.batchUninstall,
        danger: true,
        checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false },
      });
      if (!ok) return;
      setBatchTotal(queue.length);
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
              // FE-P0a: remove finished keys without clearing failed multi selections.
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
        );
      } catch (e) {
        setError(formatError(e, "cleanup"));
      }
    },
    [apps, multi, L, setMulti, setError, busyRef, batchUseOfficial, batching],
  );

  const cancelBatch = useCallback(() => {
    batchCancelRef.current = true;
    toast.info(L.batchCancelHint);
  }, [L]);

  const retryFailedBatch = useCallback(() => {
    const failed = batchResults.filter((r) => r.status === "failed");
    if (!failed.length) return;
    setMulti(new Set(failed.map((r) => r.key)));
    setBatchResults([]);
    setShowBatchSummary(false);
  }, [batchResults, setMulti]);

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
    execReal,
    handleCleanupConfirm,
    batchCleanup,
    cancelBatch,
    retryFailedBatch,
  };
}
