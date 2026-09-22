import { useCallback, useEffect, useRef } from "react";
import { api } from "../lib/api";
import type { CleanupReport, FullCleanupReport, InstalledApp, ScanResult } from "../types";
import { t } from "../i18n";
import { formatError, prettyAppName } from "../lib/format";
import { requestConfirm, requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { defaultSelectable } from "../lib/decision";
import { appKey } from "../lib/appKey";
import { loadRescanAfterUninstall, saveRescanAfterUninstall } from "../lib/rescanPref";
import type { IgnoreSuggestion } from "../types";
import type { UninstallStage } from "../components/UninstallStageBar";

/** Grouped setters for the analyze/uninstall flow (A-4: fewer flat parameters). */
export type AnalyzeFlowSetters = {
  setSelected: (a: InstalledApp | null) => void;
  setScanning: (v: boolean) => void;
  setScan: (r: ScanResult | null) => void;
  setReport: (r: CleanupReport | FullCleanupReport | null) => void;
  setAiNotes: (m: Record<string, string>) => void;
  setAiRisk: (v: string | null) => void;
  setIgnoreSuggestions: (s: IgnoreSuggestion[]) => void;
  setSelectedPaths: (s: Set<string>) => void;
  setError: (e: string | null) => void;
  setResidualFromUninstall: (v: boolean) => void;
  setUninstallingKey: (k: string | null) => void;
  setUninstallStage: (s: UninstallStage) => void;
};

/** Deep-analyze + official uninstall flow used by list rows and detail drawer. */
export function useAnalyzeFlow({
  goNav,
  flow,
  refreshApps,
  busyRef,
}: {
  goNav: (n: "software") => void;
  flow: AnalyzeFlowSetters;
  refreshApps: () => Promise<void>;
  busyRef: { current: boolean };
}) {
  const {
    setSelected,
    setScanning,
    setScan,
    setReport,
    setAiNotes,
    setAiRisk,
    setIgnoreSuggestions,
    setSelectedPaths,
    setError,
    setResidualFromUninstall,
    setUninstallingKey,
    setUninstallStage,
  } = flow;
  const analyzeSeqRef = useRef(0);
  const analyzingRef = useRef(false);
  /** F-R6-11: stage idle timers must be cleared on unmount / replacement. */
  const stageTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clearStageTimer = useCallback(() => {
    if (stageTimerRef.current !== null) {
      clearTimeout(stageTimerRef.current);
      stageTimerRef.current = null;
    }
  }, []);
  useEffect(() => clearStageTimer, [clearStageTimer]);
  const analyze = useCallback(
    async (app: InstalledApp, opts?: { fromUninstall?: boolean }) => {
      // only the newest request may write scan state, and only it may clear the spinner.
      const seq = ++analyzeSeqRef.current;
      analyzingRef.current = true;
      goNav("software");
      if (!opts?.fromUninstall) setResidualFromUninstall(false);
      setSelected(app);
      setScanning(true);
      setScan(null);
      setReport(null);
      setAiNotes({});
      setAiRisk(null);
      setIgnoreSuggestions([]);
      const t0 = performance.now();
      try {
        const r = await api.analyze(app);
        if (seq !== analyzeSeqRef.current) return;
        setScan(r);
        const sharedPaths = r.items.filter((it) => it.shared).map((it) => it.path);
        if (sharedPaths.length > 0 && app.publisher) {
          void api
            .suggestIgnoreRules(app.publisher, sharedPaths)
            .then((sugs) => {
              if (seq === analyzeSeqRef.current) setIgnoreSuggestions(sugs || []);
            })
            .catch(() => {});
        }
        setSelectedPaths(new Set(r.items.filter(defaultSelectable).map((it) => it.path)));
        setError(null);
        toast.success(
          t().toastAnalyzeDone(((performance.now() - t0) / 1000).toFixed(1), r.items.length),
          { channel: "analyze-flow", ttl: 3000 },
        );
      } catch (e) {
        if (seq !== analyzeSeqRef.current) return;
        setError(formatError(e, "analyze"));
        toast.error(formatError(e, "analyze"), { channel: "analyze-flow" });
      } finally {
        if (seq === analyzeSeqRef.current) {
          analyzingRef.current = false;
          setScanning(false);
        }
      }
    },
    [
      goNav,
      setSelected,
      setScanning,
      setScan,
      setReport,
      setAiNotes,
      setAiRisk,
      setIgnoreSuggestions,
      setSelectedPaths,
      setError,
      setResidualFromUninstall,
    ],
  );

  const startUninstall = useCallback(
    async (app: InstalledApp) => {
      if (busyRef.current) return;
      const strings = t();
      const label = prettyAppName(app.name, app.source);
      const steps = [
        strings.uninstallFlowStepOfficial,
        strings.uninstallFlowStepScan,
        strings.uninstallFlowStepAnalyze,
        strings.uninstallFlowStepConfirm,
        strings.uninstallFlowStepBackup,
      ];
      const defaultRescan = loadRescanAfterUninstall();
      const { ok, checked } = await requestConfirmEx({
        title: strings.uninstallConfirmDeepTitle || strings.uninstallFlowDeep,
        message: `${strings.uninstallConfirmDeepBody(label)}\n\n${steps.map((s, i) => `${i + 1}. ${s}`).join("\n")}`,
        confirmLabel: strings.drawerDeepUninstall,
        checkbox: {
          label: strings.uninstallRescanAfter,
          defaultChecked: defaultRescan,
        },
      });
      if (!ok) return;
      if (busyRef.current) return;
      saveRescanAfterUninstall(checked);
      const key = appKey(app);
      setUninstallingKey(key);
      setSelected(app);
      setResidualFromUninstall(false);
      busyRef.current = true;
      try {
        setUninstallStage("identify");
        await new Promise((r) => setTimeout(r, 280));
        setUninstallStage("official");
        const r = await api.officialUninstall(app);
        if (!r.had_command) {
          toast.info(strings.uninstallNoCmd);
        } else if (r.ok) {
          toast.success(strings.uninstallOk);
        } else {
          toast.error(`${strings.uninstallFail}: ${r.message}`);
        }
        if (r.had_command && r.ok && checked) {
          setUninstallStage("scan");
          toast.info(strings.stageScanLeftover, { channel: "analyze-flow", sticky: true });
          await analyze(app, { fromUninstall: true });
          setUninstallStage("analyze");
          await new Promise((resolve) => setTimeout(resolve, 320));
          setUninstallStage("report");
          clearStageTimer();
          stageTimerRef.current = setTimeout(() => setUninstallStage("idle"), 2200);
        } else {
          setUninstallStage("idle");
        }
        await refreshApps();
      } catch (e) {
        clearStageTimer();
        setUninstallStage("idle");
        setError(formatError(e, "cleanup"));
        toast.error(formatError(e, "cleanup"), { channel: "analyze-flow" });
      } finally {
        busyRef.current = false;
        setUninstallingKey(null);
      }
    },
    [
      analyze,
      refreshApps,
      setSelected,
      setResidualFromUninstall,
      setUninstallingKey,
      setUninstallStage,
      setError,
      busyRef,
      clearStageTimer,
    ],
  );

  const openDeepFromDrawer = useCallback(
    (app: InstalledApp) => {
      void startUninstall(app);
    },
    [startUninstall],
  );

  const openOfficialOnly = useCallback(
    async (app: InstalledApp) => {
      if (busyRef.current) return;
      const strings = t();
      const label = prettyAppName(app.name, app.source);
      const ok = await requestConfirm({
        title: strings.drawerOfficial,
        message: strings.uninstallConfirm(label),
        confirmLabel: strings.drawerOfficial,
      });
      if (!ok) return;
      if (busyRef.current) return;
      const key = appKey(app);
      setUninstallingKey(key);
      setSelected(app);
      busyRef.current = true;
      try {
        const r = await api.officialUninstall(app);
        if (!r.had_command) toast.info(strings.uninstallNoCmd);
        else if (r.ok) toast.success(strings.uninstallOk);
        else toast.error(`${strings.uninstallFail}: ${r.message}`);
        // FN-04: default-on rescan after successful official uninstall only.
        if (r.had_command && r.ok && loadRescanAfterUninstall()) {
          setUninstallStage("scan");
          await analyze(app, { fromUninstall: true });
          setUninstallStage("analyze");
          await new Promise((resolve) => setTimeout(resolve, 280));
          setUninstallStage("report");
          clearStageTimer();
          stageTimerRef.current = setTimeout(() => setUninstallStage("idle"), 2000);
        }
        await refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
        toast.error(formatError(e, "cleanup"));
      } finally {
        busyRef.current = false;
        setUninstallingKey(null);
      }
    },
    [refreshApps, setSelected, setUninstallingKey, setError, busyRef, analyze, setUninstallStage, clearStageTimer],
  );

  const openAnalyzeFromDrawer = useCallback(
    (app: InstalledApp) => {
      if (analyzingRef.current) return;
      void analyze(app);
    },
    [analyze],
  );

  const selectApp = useCallback(
    (app: InstalledApp) => {
      setSelected(app);
    },
    [setSelected],
  );

  return {
    analyze,
    startUninstall,
    openDeepFromDrawer,
    openOfficialOnly,
    openAnalyzeFromDrawer,
    selectApp,
  };
}
