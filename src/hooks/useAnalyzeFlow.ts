import { useCallback } from "react";
import { api } from "../lib/api";
import type { CleanupReport, FullCleanupReport, InstalledApp, ScanResult } from "../types";
import { t } from "../i18n";
import { formatError, prettyAppName } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { defaultSelectable } from "../lib/decision";
import { appKey } from "../lib/appKey";
import { loadRescanAfterUninstall } from "../lib/rescanPref";
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
  const analyze = useCallback(
    async (app: InstalledApp, opts?: { fromUninstall?: boolean }) => {
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
        setScan(r);
        const sharedPaths = r.items.filter((it) => it.shared).map((it) => it.path);
        if (sharedPaths.length > 0 && app.publisher) {
          void api
            .suggestIgnoreRules(app.publisher, sharedPaths)
            .then((sugs) => setIgnoreSuggestions(sugs || []))
            .catch(() => {});
        }
        setSelectedPaths(new Set(r.items.filter(defaultSelectable).map((it) => it.path)));
        setError(null);
        toast.success(
          t().toastAnalyzeDone(((performance.now() - t0) / 1000).toFixed(1), r.items.length),
        );
      } catch (e) {
        setError(formatError(e, "analyze"));
        toast.error(t().errAnalyzeFailed(formatError(e, "analyze")));
      } finally {
        setScanning(false);
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
      const strings = t();
      const label = prettyAppName(app.name, app.source);
      const ok = await requestConfirm({
        title: strings.uninstallFlowDeep,
        message: `${strings.uninstallConfirm(label)}\n\n${strings.drawerDeepHint}`,
        confirmLabel: strings.drawerDeepUninstall,
      });
      if (!ok) return;
      const key = appKey(app);
      setUninstallingKey(key);
      setSelected(app);
      setResidualFromUninstall(false);
      busyRef.current = true;
      try {
        setUninstallStage("identify");
        toast.info(strings.stageIdentify);
        await new Promise((r) => setTimeout(r, 400));
        setUninstallStage("official");
        toast.info(strings.stageOfficial);
        const r = await api.officialUninstall(app);
        if (!r.had_command) {
          toast.info(strings.uninstallNoCmd);
        } else if (r.ok) {
          toast.success(strings.uninstallOk);
        } else {
          toast.error(`${strings.uninstallFail}: ${r.message}`);
        }
        if (r.had_command && r.ok) {
          setUninstallStage("scan");
          toast.info(strings.stageScanLeftover);
          await analyze(app, { fromUninstall: true });
          setUninstallStage("report");
          setTimeout(() => setUninstallStage("idle"), 2500);
        } else {
          setUninstallStage("idle");
        }
        await refreshApps();
      } catch (e) {
        setUninstallStage("idle");
        setError(formatError(e, "cleanup"));
        toast.error(strings.errCleanupFailed(formatError(e, "cleanup")));
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
      const strings = t();
      const label = prettyAppName(app.name, app.source);
      const ok = await requestConfirm({
        title: strings.drawerOfficial,
        message: strings.uninstallConfirm(label),
        confirmLabel: strings.drawerOfficial,
      });
      if (!ok) return;
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
          setUninstallStage("report");
          setTimeout(() => setUninstallStage("idle"), 2000);
        }
        await refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
        toast.error(strings.errCleanupFailed(formatError(e, "cleanup")));
      } finally {
        busyRef.current = false;
        setUninstallingKey(null);
      }
    },
    [refreshApps, setSelected, setUninstallingKey, setError, busyRef, analyze, setUninstallStage],
  );

  const openAnalyzeFromDrawer = useCallback(
    (app: InstalledApp) => {
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
