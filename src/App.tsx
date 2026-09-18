import { api } from "./lib/api";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "./types";
import { loadLang, t } from "./i18n";
import { cssStyles as css, globalCss } from "./styles";
import { formatError } from "./lib/format";
import { applyTheme } from "./lib/theme";
import { Shell } from "./components/Shell";
import { ConfirmHost } from "./components/ui/ConfirmHost";
import { CloseChoiceHost } from "./components/ui/CloseChoiceHost";
import { ToastHost } from "./components/ui/ToastHost";
import { AppDetailPanel } from "./components/AppDetailPanel";
import { toast } from "./lib/toast";
import { isRecentInstall, summarizeLeftovers } from "./lib/decision";
import type { LinkedBucketId } from "./lib/linkedItems";
import { appKey } from "./lib/appKey";
import { useSizeEstimate } from "./hooks/useSizeEstimate";
import { useAppFilter } from "./hooks/useAppFilter";
import { usePendingAnalyze, useDragDropAnalyze } from "./hooks/useAppNavAssist";
import { useCleanupHandlers } from "./hooks/useCleanupHandlers";
import { useAnalyzeFlow } from "./hooks/useAnalyzeFlow";
import { useAppBoot, checkUpdateNow, toggleShellMenuApi } from "./hooks/useAppBoot";
import { useAiPanelState } from "./hooks/useAiPanelState";
import { useShellState } from "./hooks/useShellState";
import { useListFilterChrome } from "./hooks/useListFilterChrome";
import { useResidualState } from "./hooks/useResidualState";
import { useScanUiState } from "./hooks/useScanUiState";
import { useAppCoreState } from "./hooks/useAppCoreState";
import { ErrorBanner } from "./components/StatusBanners";
import { ShellStatus, ShellFooter } from "./components/ShellChrome";
import { exportHtmlReport } from "./lib/exportHtmlReport";
import { runAiReportSummary } from "./lib/aiNarrative";
import { loadRescanAfterUninstall } from "./lib/rescanPref";
import { type CloseMode } from "./lib/closeMode";
import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, Suspense, lazy } from "react";

/** PF-08: code-split heavy nav pages. */
const SoftwarePage = lazy(() =>
  import("./components/SoftwarePage").then((m) => ({ default: m.SoftwarePage })),
);
const ManageListPage = lazy(() =>
  import("./components/ManageListPage").then((m) => ({ default: m.ManageListPage })),
);
const MorePage = lazy(() => import("./components/MorePage").then((m) => ({ default: m.MorePage })));
const OrphanPage = lazy(() =>
  import("./components/OrphanPage").then((m) => ({ default: m.OrphanPage })),
);

declare const __APP_VERSION__: string;

function useCheckupStats(apps: InstalledApp[], sizeOf: (a: InstalledApp) => number, sizeMap: Record<string, number>) {
  return useMemo(() => {
    const large = apps.filter((a) => {
      const kb = sizeMap?.[a.install_location] || sizeOf(a);
      return kb > 500 * 1024;
    }).length;
    const recent = apps.filter((a) => isRecentInstall(a.install_date, 30)).length;
    return { total: apps.length, large, recent };
  }, [apps, sizeOf, sizeMap]);
}

export default function App() {
  const core = useAppCoreState();
  const {
    apps,
    loading,
    error,
    selected,
    multi,
    scan,
    scanning,
    report,
    useOfficial,
    admin,
    disk,
    uninstallingKey,
    ignorePub,
    ignoreName,
  } = core;

  const { q, setQ, sortCol, setSortCol, sortDesc, setSortDesc, category, setCategoryState } =
    useListFilterChrome();
  const shell = useShellState();
  const {
    theme,
    nav,
    langVer,
    setLangVer,
    shellMenu,
    setShellMenu,
    closeMode,
    updateInfo,
    setUpdateInfo,
    checkupOpen,
    checkupOrphanCount,
    setCheckupOrphanCount,
    uninstallStage,
    setUninstallStage,
    showDetail,
    actions: shellActions,
  } = shell;
  const setCloseMode = useCallback(
    (m: CloseMode) => {
      shellActions.persistCloseMode(m);
    },
    [shellActions],
  );
  const goNav = shellActions.goNav;

  const residual = useResidualState();
  const {
    selectedPaths,
    setSelectedPaths,
    evidence,
    setEvidence,
    ignoreSuggestions,
    lastReport,
    monitoring,
    monitorDiff,
    residualFromUninstall,
    actions: residualActions,
  } = residual;

  const ai = useAiPanelState();
  const {
    aiEnabled,
    setAiEnabled,
    aiBusy,
    setAiBusy,
    aiNotes,
    setAiNotes,
    aiRisk,
    setAiRisk,
    aiReportNote,
    setAiReportNote,
    aiReportBusy,
    setAiReportBusy,
    verifyRows,
    setVerifyRows,
    copilotList,
    setCopilotList,
    actions: aiActions,
  } = ai;

  const setCategory = useCallback(
    (id: "all" | "desktop" | "store" | "large" | "recent") => {
      setCategoryState(id);
      setCopilotList(null);
      localStorage.setItem("remova_cat", id);
    },
    [setCategoryState, setCopilotList],
  );
  const busyRef = useRef(false);
  const analyzeRef = useRef<
    (app: InstalledApp, opts?: { fromUninstall?: boolean }) => Promise<void>
  >(async () => {});
  const scanUiState = useScanUiState();
  const {
    kindFilter,
    setKindFilter,
    riskFilter,
    setRiskFilter,
    aiSummaryNote,
    setAiSummaryNote,
    aiNudgeDismissed,
    actions: scanUi,
  } = scanUiState;
  const dismissAiNudge = scanUi.dismissAiNudge;

  // langVer forces t() after language switch (module dictionary is not reactive).
  const L = useMemo(() => {
    void langVer;
    return t();
  }, [langVer]);
  const deferredQ = useDeferredValue(q);

  const { estimating, sizeMap, sizeProgress, stopSizeEstimate, sizeOf, formatAppSize } =
    useSizeEstimate(apps, loading);

  const { filtered, sortBy } = useAppFilter({
    apps,
    copilotList,
    deferredQ,
    category,
    sortCol,
    sortDesc,
    sizeOf,
    ignorePub,
    ignoreName,
    sizeMap,
    setSortCol,
    setSortDesc,
  });

  const refreshApps = useCallback(async () => {
    try {
      const list = await api.listApps();
      core.setApps(list);
    } catch (e) {
      core.setError(formatError(e));
    }
  }, [core]);

  const {
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
  } = useCleanupHandlers({
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
    flow: {
      setMulti: core.setMulti,
      setResidualFromUninstall: residualActions.setResidualFromUninstall,
      setAiRisk,
      setReport: core.setReport,
      setLastReport: residualActions.setLastReport,
      setVerifyRows,
      setAiReportNote,
      setError: core.setError,
    },
    refreshApps,
    busyRef,
  });

  const {
    analyze,
    startUninstall,
    openDeepFromDrawer,
    openOfficialOnly,
    openAnalyzeFromDrawer,
    selectApp,
  } = useAnalyzeFlow({
    goNav,
    flow: {
      setSelected: core.setSelected,
      setScanning: core.setScanning,
      setScan: core.setScan,
      setReport: core.setReport,
      setAiNotes,
      setAiRisk,
      setIgnoreSuggestions: residualActions.setIgnoreSuggestions,
      setSelectedPaths,
      setError: core.setError,
      setResidualFromUninstall: residualActions.setResidualFromUninstall,
      setUninstallingKey: core.setUninstallingKey,
      setUninstallStage,
    },
    refreshApps,
    busyRef,
  });

  const checkup = useCheckupStats(apps, sizeOf, sizeMap);

  useAppBoot({
    setApps: core.setApps,
    setLoading: core.setLoading,
    setError: core.setError,
    setAdmin: core.setAdmin,
    setAiEnabled,
    setIgnorePub: core.setIgnorePub,
    setIgnoreName: core.setIgnoreName,
    setDisk: core.setDisk,
    setUpdateInfo,
    busyRef,
  });

  useEffect(() => {
    loadLang();
    applyTheme(theme);
    setLangVer((v) => v + 1);
  }, [theme, setLangVer]);

  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (batching || dryRunning) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [batching, dryRunning]);

  usePendingAnalyze({ loading, apps, goNav, setSelected: core.setSelected, analyze, setQ });
  useDragDropAnalyze({ apps, setSelected: core.setSelected, analyze });

  const toggleMulti = (key: string) => {
    core.toggleMulti(key);
  };

  const doIgnorePublisher = useCallback(
    async (appOverride?: InstalledApp) => {
      const pub = (appOverride ?? selected)?.publisher;
      if (!pub) return;
      try {
        const ig = await api.ignorePublisher(pub);
        core.setIgnorePub(ig.publishers || []);
        toast.success(L.ignoreLoaded);
      } catch (e) {
        core.setError(formatError(e));
      }
    },
    [selected, L, core],
  );

  const doIgnoreApp = useCallback(
    async (appOverride?: InstalledApp) => {
      const name = (appOverride ?? selected)?.name;
      if (!name) return;
      try {
        const ig = await api.ignoreAppName(name);
        core.setIgnoreName(ig.names || []);
        toast.success(L.ignoreLoaded);
      } catch (e) {
        core.setError(formatError(e));
      }
    },
    [selected, L, core],
  );

  const runOrphanScan = useCallback(async () => {
    toast.info(L.orphanScanning);
    try {
      const items = await api.orphanScan();
      goNav("software");
      core.setScan({ app_name: L.orphanScan, items });
      residualActions.clearSelection();
      if (items.length === 0) toast.info(L.orphanScanEmpty);
      else {
        const s = summarizeLeftovers(items);
        toast.success(L.orphanScanDone(s.total, s.suggest, s.keep));
      }
    } catch (e) {
      core.setError(formatError(e, "analyze"));
      toast.error(L.errAnalyzeFailed(formatError(e, "analyze")));
    }
  }, [L, goNav, residualActions, core]);

  const toggleMonitor = useCallback(async () => {
    try {
      if (!monitoring) {
        await api.beginInstallMonitor();
        residualActions.setMonitoring(true);
        residualActions.setMonitorDiff(null);
        toast.info(L.monitorRunning);
      } else {
        const d = await api.endInstallMonitor();
        residualActions.setMonitoring(false);
        residualActions.setMonitorDiff(d);
        toast.success(L.toastMonitorDiff(d.added_files.length, d.added_reg_values.length));
      }
    } catch (e) {
      core.setError(formatError(e));
      toast.error(L.errInvokeFailed(formatError(e)));
      residualActions.setMonitoring(false);
    }
  }, [monitoring, L, residualActions, core]);

  const monitorDiffToCleanup = useCallback(
    async (diff: { added_files: string[]; added_reg_values: string[] }) => {
      try {
        const items = await api.monitorDiffToItems(diff);
        if (!items.length) {
          toast.info(L.monitorNoSnap);
          return;
        }
        goNav("software");
        core.setScan({ app_name: L.monitorDiff, items });
        residualActions.selectDefaultItems(items);
        residualActions.setMonitorDiff(null);
        toast.success(`${L.monitorToCleanup}: ${items.length}`);
      } catch (e) {
        core.setError(formatError(e));
      }
    },
    [L, goNav, residualActions, core],
  );

  const runAiExplain = useCallback(async () => {
    if (!scan || !aiEnabled || aiBusy) return;
    setAiBusy(true);
    try {
      const items = scan.items.slice(0, 12).map((it) => ({
        path: it.path,
        kind: String(it.kind),
        confidence: String(it.confidence),
        risk: String(it.risk),
        reason: it.reason,
        evidence_labels: (it.evidence || []).map((e) => e.label).filter(Boolean),
      }));
      const out = await api.aiExplain(scan.app_name, selected?.publisher || "", items);
      const map: Record<string, string> = {};
      for (const o of out as { path: string; summary: string }[]) {
        map[o.path] = o.summary;
      }
      setAiNotes(map);
      const brief = (out as { summary: string }[])
        .slice(0, 3)
        .map((o) => o.summary)
        .filter(Boolean)
        .join(" ");
      setAiSummaryNote(brief || null);
      if (!out.length) toast.info(L.aiDisabledHint);
    } catch {
      toast.error(L.aiFailed);
    } finally {
      setAiBusy(false);
    }
  }, [scan, selected, aiEnabled, aiBusy, L, setAiBusy, setAiNotes, setAiSummaryNote]);

  useEffect(() => {
    scanUi.clearAiSummary();
    scanUi.clearRiskFilter();
    aiActions.clearAiScanState();
    if (scan && scan.items.length > 0 && aiEnabled) {
      void runAiExplain();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scan?.app_name, scan?.items.length, aiEnabled]);

  const runAiReport = useCallback(async () => {
    if (!report || !("deleted" in report) || !aiEnabled || aiReportBusy) return;
    setAiReportBusy(true);
    try {
      const note = await runAiReportSummary(report);
      setAiReportNote(note);
    } catch {
      // rule narrative still shown
    } finally {
      setAiReportBusy(false);
    }
  }, [report, aiEnabled, aiReportBusy, setAiReportBusy, setAiReportNote]);

  useEffect(() => {
    if (report && aiEnabled) void runAiReport();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report, aiEnabled]);

  const elevate = useCallback(async () => {
    try {
      await api.elevateRestart();
    } catch (e) {
      toast.error(formatError(e, "elevate"));
    }
  }, []);

  const closePreview = useCallback(() => {
    core.closePreviewCore();
    residualActions.setResidualFromUninstall(false);
    residualActions.clearIgnoreSuggestions();
    residualActions.clearSelection();
    scanUi.clearScanChrome();
    aiActions.clearAiScanState();
    aiActions.clearAiReport();
    void refreshApps();
  }, [refreshApps, residualActions, aiActions, scanUi, core]);

  useEffect(() => {
    const pending = scanUi.takePendingBucket();
    if (scan && pending) {
      setKindFilter(pending);
    }
    if (!scan) setKindFilter(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scan]);

  const drillDownBucket = useCallback(
    (bucket: LinkedBucketId) => {
      if (scan && scan.app_name === selected?.name) {
        setKindFilter(bucket);
      } else {
        scanUi.setPendingBucket(bucket);
        if (selected) void analyze(selected);
      }
    },
    [scan, selected, analyze, scanUi, setKindFilter],
  );

  const checkupOrphanScan = useCallback(() => {
    void (async () => {
      core.setScanning(true);
      try {
        const items = await api.orphanScan();
        setCheckupOrphanCount(items.length);
      } catch (e) {
        setCheckupOrphanCount(null);
        toast.error(formatError(e, "analyze"));
      } finally {
        core.setScanning(false);
      }
    })();
  }, [setCheckupOrphanCount, core]);

  const listStartUninstall = useCallback(
    (a: InstalledApp) => void startUninstall(a),
    [startUninstall],
  );
  const listAnalyze = useCallback((a: InstalledApp) => void analyze(a), [analyze]);
  const listForceClean = useCallback((a: InstalledApp) => void forceClean(a), [forceClean]);
  const listIgnoreApp = useCallback((a: InstalledApp) => void doIgnoreApp(a), [doIgnoreApp]);
  const listIgnorePublisher = useCallback(
    (a: InstalledApp) => void doIgnorePublisher(a),
    [doIgnorePublisher],
  );
  const openPathSafe = useCallback(async (path: string) => {
    try {
      await api.openPath(path);
    } catch (e) {
      toast.error(formatError(e));
    }
  }, []);
  const detailOnClose = useCallback(() => {
    scanUi.clearKindFilter();
    core.setSelected(null);
  }, [scanUi, core]);

  const detailPanel = selected ? (
    <AppDetailPanel
      app={selected}
      sizeText={formatAppSize(selected)}
      uninstalling={uninstallingKey === appKey(selected)}
      scan={scan}
      onClose={detailOnClose}
      onDeepUninstall={openDeepFromDrawer}
      onAnalyze={openAnalyzeFromDrawer}
      onOfficialOnly={(app) => void openOfficialOnly(app)}
      onForceClean={(app) => void forceClean(app)}
      onOpenPath={openPathSafe}
      onDrillDown={drillDownBucket}
      onViewLeftovers={() => setKindFilter(null)}
    />
  ) : null;

  const [langTick] = useState(0);
  void langTick;
  const rescanNudge = loadRescanAfterUninstall;
  void rescanNudge;
  void analyzeRef;

  return (
    <>
      <style>{globalCss}</style>
      <ConfirmHost />
      <CloseChoiceHost />
      <ToastHost />
      <Shell
        nav={nav}
        onNav={goNav}
        title={
          nav === "software"
            ? L.navSoftware
            : nav === "startup"
              ? L.navStartup
              : nav === "services"
                ? L.navServices
                : nav === "tasks"
                  ? L.navTasks
                  : nav === "orphans"
                    ? L.navOrphans
                    : L.toolboxTitle
        }
        subtitle={nav === "software" ? L.installedCount(apps.length) : undefined}
        status={
          nav === "software" ? (
            <ShellStatus disk={disk} admin={admin} onElevate={() => void elevate()} />
          ) : null
        }
        footer={
          <ShellFooter
            updateInfo={updateInfo}
            selectedCount={nav === "software" ? multi.size : undefined}
            totalCount={
              nav === "software" ? (loading ? apps.length : filtered.length) : undefined
            }
            estimating={nav === "software" ? estimating : undefined}
            estimateLabel={
              sizeProgress.total > 0
                ? L.estimateProgress(sizeProgress.done, sizeProgress.total)
                : L.estimatingSizes
            }
            monitoring={nav === "software" ? monitoring : undefined}
          />
        }
        actions={
          <>
            <button style={css.btnSm} onClick={shellActions.toggleTheme}>
              {L.themeToggle}
            </button>
            <button style={css.btnSm} onClick={shellActions.toggleLang}>
              {L.langToggle}
            </button>
          </>
        }
      >
        {error && <ErrorBanner error={error} onDismiss={() => core.setError(null)} />}
        <Suspense fallback={null}>
          {nav === "startup" && (
            <ManageListPage tab="startup" title={L.navStartup} onError={core.setError} />
          )}
          {nav === "services" && (
            <ManageListPage tab="services" title={L.navServices} onError={core.setError} />
          )}
          {nav === "tasks" && (
            <ManageListPage tab="tasks" title={L.navTasks} onError={core.setError} />
          )}
          {nav === "orphans" && (
            <OrphanPage
              onLastReport={(r: FullCleanupReport) => {
                residualActions.setLastReport(r);
                core.setReport(r);
              }}
            />
          )}
          {nav === "more" && (
            <MorePage
              selected={selected}
              monitoring={monitoring}
              monitorDiff={monitorDiff}
              lastReport={lastReport}
              shellMenu={shellMenu}
              closeMode={closeMode}
              onCloseModeChange={setCloseMode}
              onForceClean={() => void forceClean()}
              onIgnorePublisher={() => void doIgnorePublisher()}
              onOrphanScan={() => void runOrphanScan()}
              onToggleMonitor={() => void toggleMonitor()}
              onMonitorToCleanup={() => monitorDiff && void monitorDiffToCleanup(monitorDiff)}
              onDismissMonitor={() => residualActions.setMonitorDiff(null)}
              onShellToggle={() => void toggleShellMenuApi(shellMenu, setShellMenu, core.setError, L)}
              onExportReport={() => {
                if (lastReport) exportHtmlReport(lastReport, L);
              }}
              onError={core.setError}
              onCheckUpdate={() => void checkUpdateNow(setUpdateInfo, L)}
              onGoSoftware={() => goNav("software")}
            />
          )}
          {nav === "software" && (
            <SoftwarePage
              apps={apps}
              filtered={filtered}
              loading={loading}
              q={q}
              category={category}
              sortCol={sortCol}
              sortDesc={sortDesc}
              selected={selected}
              multi={multi}
              uninstallingKey={uninstallingKey}
              formatAppSize={formatAppSize}
              sizeOf={sizeOf}
              sortBy={sortBy}
              selectApp={selectApp}
              setSelected={core.setSelected}
              toggleMulti={toggleMulti}
              listStartUninstall={listStartUninstall}
              listAnalyze={listAnalyze}
              listForceClean={listForceClean}
              listIgnoreApp={listIgnoreApp}
              listIgnorePublisher={listIgnorePublisher}
              appKey={appKey}
              setMulti={core.setMulti}
              estimating={estimating}
              scanning={scanning}
              aiEnabled={aiEnabled}
              uninstallStage={uninstallStage}
              showDetail={showDetail}
              onToggleDetail={shellActions.toggleDetail}
              onQuery={(v) => {
                setQ(v);
                setCopilotList(null);
              }}
              onCategory={setCategory}
              onStopEstimate={() => void stopSizeEstimate()}
              onOpenAi={() => goNav("more")}
              scan={scan}
              selectedPaths={selectedPaths}
              evidence={evidence}
              setEvidence={setEvidence}
              setSelectedPaths={setSelectedPaths}
              ignoreSuggestions={ignoreSuggestions}
              onIgnoreApplied={(pubs, names) => {
                core.setIgnorePub(pubs);
                core.setIgnoreName(names);
                residualActions.clearIgnoreSuggestions();
              }}
              onDismissIgnore={() => residualActions.clearIgnoreSuggestions()}
              residualFromUninstall={residualFromUninstall}
              useOfficial={useOfficial}
              setUseOfficial={core.setUseOfficial}
              aiBusy={aiBusy}
              aiRisk={aiRisk}
              setAiRisk={setAiRisk}
              aiNotes={aiNotes}
              aiSummaryNote={aiSummaryNote}
              aiNudgeDismissed={aiNudgeDismissed}
              onDismissAiNudge={dismissAiNudge}
              dryRunning={dryRunning}
              busy={dryRunning || batching || scanning || aiBusy}
              onBack={closePreview}
              onDryRun={() => void dryRun()}
              onCleanup={() => void handleCleanupConfirm()}
              onAiExplain={() => void runAiExplain()}
              error={error}
              setError={core.setError}
              report={report}
              setReport={core.setReport}
              aiReportBusy={aiReportBusy}
              aiReportNote={aiReportNote}
              setAiReportBusy={setAiReportBusy}
              setAiReportNote={setAiReportNote}
              verifyRows={verifyRows}
              checkup={checkup}
              checkupOpen={checkupOpen}
              setCheckupOpen={shell.setCheckupOpen}
              checkupOrphanCount={checkupOrphanCount}
              checkupOrphanScan={checkupOrphanScan}
              onGoOrphans={() => {
                shellActions.closeCheckup();
                goNav("orphans");
              }}
              batching={batching}
              batchIndex={batchIndex}
              batchTotal={batchTotal}
              batchCurrent={batchCurrent}
              batchResults={batchResults}
              showBatchSummary={showBatchSummary}
              setShowBatchSummary={setShowBatchSummary}
              retryFailedBatch={retryFailedBatch}
              cancelBatch={cancelBatch}
              batchCleanup={() => void batchCleanup()}
              kindFilter={kindFilter}
              setKindFilter={setKindFilter}
              riskFilter={riskFilter}
              setRiskFilter={setRiskFilter}
              detailPanel={detailPanel}
              onOpenSettings={() => goNav("more")}
              onCopilotApplyFilter={(list) => {
                setCopilotList(list);
                setQ("");
                setCategoryState("all");
              }}
              onCopilotBatch={(list) => {
                core.setMulti(new Set(list.map(appKey)));
                setCopilotList(list);
                toast.info(L.batchUninstall);
              }}
            />
          )}
        </Suspense>
      </Shell>
    </>
  );
}

export type { CleanupReport, ScanResult };
