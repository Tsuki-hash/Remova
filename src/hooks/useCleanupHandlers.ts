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
  /**: invalidates an in-flight verify probe when a newer cleanup starts. */
  const verifySeqRef = useRef(0);
  /** F-R6-05: only the latest dry-run may write its report. */
  const dryRunSeqRef = useRef(0);
  /** F-R6-07: invalidates force-clean's internal analyze when a newer one starts. */
  const forceAnalyzeSeqRef = useRef(0);

  const forceClean = useCallback(
    async (appOverride?: InstalledApp) => {
      const target = appOverride ?? selected;
      if (!target || forceBusy || busyRef.current) return;
      setForceBusy(true);
      busyRef.current = true;
      const aseq = ++forceAnalyzeSeqRef.current;
      try {
        const r = await api.analyze(target);
        // F-R6-07: a newer force-clean/analyze started while this one was in flight.
        if (aseq !== forceAnalyzeSeqRef.current) return;
        const items = r.items.filter(defaultSelectable);
        if (!items.length) {
          toast.info(L.toastForceCleanEmpty);
          return;
        }
        // F-R6-04: surface user_library / shared risks in the force-clean confirm too.
        const forceRiskBits: string[] = [];
        if (items.some((it) => it.risk === "high")) forceRiskBits.push(L.conclusionHighRiskHint);
        if (items.some((it) => it.user_data)) forceRiskBits.push(L.conclusionUserDataHint);
        if (items.some((it) => it.user_library)) forceRiskBits.push(L.confirmUserLibrarySelected);
        if (items.some((it) => it.shared)) forceRiskBits.push(L.confirmSharedSelected);
        const forceRiskNote = forceRiskBits.length ? `\n\n⚠ ${forceRiskBits.join("\n")}` : "";
        const { ok, checked } = await requestConfirmEx({
          title: L.forceClean,
          message: `${prettyAppName(target.name, target.source)}\n${L.confirmForceRiskPrefix(riskTierLabel(maxRiskOf(items), L))}${forceRiskNote}\n${L.forceCleanHint}`,
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
          setReport(report);
          setVerifyRows(null);
          setAiReportNote(null);
          return;
        }
        toast.success(
          `${L.forceClean}: ${prettyAppName(target.name, target.source)} · ${L.batchDetail(report.deleted, report.failed)}`,
        );
        setReport(report);
        void refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
        toast.error(formatError(e, "cleanup"));
      } finally {
        busyRef.current = false;
        setForceBusy(false);
      }
    },
    [selected, forceBusy, L, refreshApps, setError, setReport, busyRef, setVerifyRows, setAiReportNote],
  );

  const dryRun = useCallback(async () => {
    if (!scan || !selected) return;
    // F-R6-05: never overlap dry-run with another busy cleanup/uninstall.
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
    } catch (e) {
      if (seq !== dryRunSeqRef.current) return;
      setError(formatError(e, "cleanup"));
    } finally {
      if (seq === dryRunSeqRef.current) setDryRunning(false);
      busyRef.current = false;
    }
  }, [scan, selected, selectedPaths, setReport, setError, busyRef]);

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
      if (r.aborted) {
        const msg = r.uninstall_message || L.errCleanupFailed("aborted");
        toast.error(msg);
        setError(msg);
        setVerifyRows(null);
        setAiReportNote(null);
        setResidualFromUninstall(false);
        void refreshApps();
        return;
      }
      setAiReportNote(null);
      setVerifyRows(null);
      if (r.failed > 0) {
        toast.error(L.batchDetail(r.deleted, r.failed));
      } else {
        toast.success(L.batchDetail(r.deleted, r.failed));
      }
      // SOP checklist: re-probe cleaned paths
      const vseq = ++verifySeqRef.current;
      void api
        .verifyLeftovers(items)
        .then((rows) => {
          if (vseq === verifySeqRef.current) setVerifyRows(rows);
        })
        .catch(() => {});
      if (r.uninstall_ok && selected) {
        onAfterCleanup?.(selected, r);
      }
      setResidualFromUninstall(false);
      void refreshApps();
    } catch (e) {
      setError(formatError(e, "cleanup"));
      toast.error(formatError(e, "cleanup"));
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
    setAiReportNote,
    setVerifyRows,
    setResidualFromUninstall,
    setError,
    busyRef,
  ]);

  /** Confirm vault + compressed key risks (full narrative lives in scan conclusion). */
  const handleCleanupConfirm = useCallback(async () => {
    if (!scan) return;
    if (busyRef.current) {
      toast.info(L.errCleanupFailed("busy"));
      return;
    }
    // Hold busyRef across AI brief + confirm so execReal cannot silently no-op.
    busyRef.current = true;
    try {
    const n = selectedPaths.size;
    if (n === 0) {
      // FE-N3: never confirm an empty cleanup set.
      toast.info(L.cleanup);
      busyRef.current = false;
      return;
    }
    const picked = scan.items.filter((it) => selectedPaths.has(it.path));
    let message = `${L.riskTierPrefix(riskTierLabel(maxRiskOf(picked), L))}\n${L.cleanupConfirmOptionalBackup(
      n,
      residualFromUninstall || useOfficial,
    )}`;
    const riskBits: string[] = [];
    if (picked.some((it) => it.risk === "high")) riskBits.push(L.conclusionHighRiskHint);
    if (picked.some((it) => it.user_data)) riskBits.push(L.conclusionUserDataHint);
    if (picked.some((it) => it.user_library)) riskBits.push(L.confirmUserLibrarySelected);
    if (picked.some((it) => it.shared)) riskBits.push(L.confirmSharedSelected);
    if (riskBits.length) {
      // Never truncate: high-risk must stay visible (F-R6-01).
      message = `${message}\n\n⚠ ${riskBits.join("\n")}`;
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
    retryFailedBatch,
  };
}
