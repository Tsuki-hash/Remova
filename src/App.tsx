import { api } from "./lib/api";
import type { FullCleanupReport, InstalledApp } from "./types";
import { loadLang, t } from "./i18n";
import { cssStyles as css, globalCss } from "./styles";
import { formatError } from "./lib/format";
import { applyTheme } from "./lib/theme";
import { navSubtitle, navTitle } from "./lib/appNav";
import { Shell } from "./components/Shell";
import { ConfirmHost } from "./components/ui/ConfirmHost";
import { CloseChoiceHost } from "./components/ui/CloseChoiceHost";
import { ToastHost } from "./components/ui/ToastHost";
import { AppDetailPanel } from "./components/AppDetailPanel";
import type { LinkedBucketId } from "./lib/linkedItems";
import { appKey } from "./lib/appKey";
import { useSizeEstimate } from "./hooks/useSizeEstimate";
import { useAppFilter } from "./hooks/useAppFilter";
import { usePendingAnalyze, useDragDropAnalyze } from "./hooks/useAppNavAssist";
import { useCleanupHandlers } from "./hooks/useCleanupHandlers";
import { useAnalyzeFlow } from "./hooks/useAnalyzeFlow";
import { useAppBoot, checkUpdateNow } from "./hooks/useAppBoot";
import { useAiPanelState } from "./hooks/useAiPanelState";
import { useShellState } from "./hooks/useShellState";
import { useListFilterChrome } from "./hooks/useListFilterChrome";
import { useResidualState } from "./hooks/useResidualState";
import { useScanUiState } from "./hooks/useScanUiState";
import { useAppCoreState } from "./hooks/useAppCoreState";
import { useAppChrome, useCheckupStats } from "./hooks/useAppChrome";
import { useAiScanNarrative } from "./hooks/useAiScanNarrative";
import { ErrorBanner } from "./components/StatusBanners";
import { ShellStatus, ShellFooter } from "./components/ShellChrome";
import { exportHtmlReport } from "./lib/exportHtmlReport";
import { loadRescanAfterUninstall } from "./lib/rescanPref";
import { type CloseMode } from "./lib/closeMode";
import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  Suspense,
  lazy,
} from "react";
import { useSoftwareController } from "./hooks/useSoftwareController";

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
    lastReport,
    useOfficial,
    admin,
    disk,
    uninstallingKey,
    ignorePub,
    ignoreName,
    // reducer setters are individually stable; alias them so memo deps stay constant.
    setSelected: coreSetSelected,
    setMulti: coreSetMulti,
    setIgnorePub: coreSetIgnorePub,
    setIgnoreName: coreSetIgnoreName,
    setUseOfficial: coreSetUseOfficial,
    setError: coreSetError,
    setReport: coreSetReport,
    toggleMulti: coreToggleMulti,
    closePreviewCore: coreClosePreview,
  } = core;

  const { q, setQ, sortCol, setSortCol, sortDesc, setSortDesc, category, setCategoryState } =
    useListFilterChrome();
  const shell = useShellState();
  const {
    theme,
    nav,
    langVer,
    setLangVer,
    closeMode,
    updateInfo,
    setUpdateInfo,
    checkupOpen,
    checkupOrphanCount,
    uninstallStage,
    setUninstallStage,
    showDetail,
    setCheckupOpen: shellSetCheckupOpen,
    actions: shellActions,
  } = shell;
  const setCloseMode = useCallback(
    (m: CloseMode) => {
      shellActions.persistCloseMode(m);
    },
    [shellActions],
  );
  const goNav = shellActions.goNav;

  // stable action bags for the software controller — the whole `core`/`shell` objects
  // change identity on every unrelated state update and would defeat SoftwarePage's memo.
  const coreActions = useMemo(
    () => ({
      setSelected: coreSetSelected,
      setMulti: coreSetMulti,
      setIgnorePub: coreSetIgnorePub,
      setIgnoreName: coreSetIgnoreName,
      setUseOfficial: coreSetUseOfficial,
      setError: coreSetError,
      setReport: coreSetReport,
    }),
    [
      coreSetSelected,
      coreSetMulti,
      coreSetIgnorePub,
      coreSetIgnoreName,
      coreSetUseOfficial,
      coreSetError,
      coreSetReport,
    ],
  );
  const shellCheckup = useMemo(
    () => ({ setCheckupOpen: shellSetCheckupOpen }),
    [shellSetCheckupOpen],
  );

  const residual = useResidualState();
  const {
    selectedPaths,
    setSelectedPaths,
    evidence,
    setEvidence,
    ignoreSuggestions,
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

  const { estimating, sizeProgress, stopSizeEstimate, sizeOf, formatAppSize } =
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
    setSortCol,
    setSortDesc,
  });

  const coreSetApps = core.setApps;
  const refreshApps = useCallback(async () => {
    try {
      const list = await api.listApps();
      coreSetApps(list);
    } catch (e) {
      coreSetError(formatError(e));
    }
  }, [coreSetApps, coreSetError]);

  const onAfterCleanup = useCallback(
    (app: InstalledApp) => {
      // FN-04: re-analyze after cleanup when user preference is on.
      if (!loadRescanAfterUninstall()) return;
      void analyzeRef.current(app, { fromUninstall: true });
    },
    [],
  );

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
      setVerifyRows,
      setAiReportNote,
      setError: core.setError,
    },
    refreshApps,
    onAfterCleanup,
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
      setEvidence,
    },
    refreshApps,
    busyRef,
  });

  useEffect(() => {
    analyzeRef.current = analyze;
  }, [analyze]);

  const checkup = useCheckupStats(apps, sizeOf);

  // F-R7-03: chrome/tool actions live in useAppChrome; AI narrative in useAiScanNarrative.
  const {
    checkupOrphanBusy,
    elevate,
    openPathSafe,
    doIgnorePublisher,
    doIgnoreApp,
    toggleMonitor,
    monitorDiffToCleanup,
    checkupOrphanScan,
  } = useAppChrome({
    L,
    selected,
    monitoring,
    goNav,
    flow: {
      setIgnorePub: core.setIgnorePub,
      setIgnoreName: core.setIgnoreName,
      setError: core.setError,
      setScan: core.setScan,
      setSelected: core.setSelected,
    },
    residual: {
      clearSelection: residualActions.clearSelection,
      setMonitoring: residualActions.setMonitoring,
      setMonitorDiff: residualActions.setMonitorDiff,
      monitorDiff,
      selectDefaultItems: residualActions.selectDefaultItems,
    },
    setCheckupOrphanCount: shell.setCheckupOrphanCount,
  });

  const { runAiExplain } = useAiScanNarrative({
    scan,
    selected,
    report,
    aiEnabled,
    aiBusy,
    aiReportBusy,
    L,
    setAiBusy,
    setAiNotes,
    setAiSummaryNote,
    setAiReportBusy,
    setAiReportNote,
    scanUi: { clearAiSummary: scanUi.clearAiSummary, clearRiskFilter: scanUi.clearRiskFilter },
    aiActions: { clearAiScanState: aiActions.clearAiScanState },
  });

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

  usePendingAnalyze({
    loading,
    apps,
    goNav,
    setSelected: core.setSelected,
    analyze: (a) => {
      void analyze(a);
    },
    setQ,
  });
  useDragDropAnalyze({
    apps,
    setSelected: core.setSelected,
    analyze: (a) => {
      void analyze(a);
    },
  });

  const toggleMulti = useCallback(
    (key: string) => {
      coreToggleMulti(key);
    },
    [coreToggleMulti],
  );

  const closePreview = useCallback(() => {
    coreClosePreview();
    coreActions.setUseOfficial(false);
    residualActions.setResidualFromUninstall(false);
    residualActions.clearIgnoreSuggestions();
    residualActions.clearSelection();
    setEvidence(null);
    scanUi.clearScanChrome();
    aiActions.clearAiScanState();
    aiActions.clearAiReport();
    setShowBatchSummary(false);
    void refreshApps();
  }, [
    refreshApps,
    residualActions,
    aiActions,
    scanUi,
    coreClosePreview,
    coreActions,
    setShowBatchSummary,
  ]);

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
      // REV-FE-04: same in-flight guard as listAnalyze (scanningRef mirrors analyzing).
      if (scanning) return;
      if (scan && scan.app_name === selected?.name) {
        setKindFilter(bucket);
      } else {
        scanUi.setPendingBucket(bucket);
        if (selected) void analyze(selected);
      }
    },
    [scan, selected, analyze, scanUi, setKindFilter, scanning],
  );

  // a scan in progress absorbs new row-analyze requests (REV-FE-04: one guard = `scanning`).
  const listStartUninstall = useCallback(
    (a: InstalledApp) => void startUninstall(a),
    [startUninstall],
  );
  const listAnalyze = useCallback(
    (a: InstalledApp) => {
      if (scanning) return;
      void analyze(a);
    },
    [analyze, scanning],
  );
  const listForceClean = useCallback((a: InstalledApp) => void forceClean(a), [forceClean]);
  const listIgnoreApp = useCallback((a: InstalledApp) => void doIgnoreApp(a), [doIgnoreApp]);
  const listIgnorePublisher = useCallback(
    (a: InstalledApp) => void doIgnorePublisher(a),
    [doIgnorePublisher],
  );
  const detailOnClose = useCallback(() => {
    scanUi.clearKindFilter();
    coreSetSelected(null);
  }, [scanUi, coreSetSelected]);

  const detailPanel = useMemo(
    () =>
      selected ? (
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
          onOpenPath={(p) => {
            void openPathSafe(p);
          }}
          onDrillDown={drillDownBucket}
          onViewLeftovers={() => setKindFilter(null)}
        />
      ) : null,
    [
      selected,
      formatAppSize,
      uninstallingKey,
      scan,
      detailOnClose,
      openDeepFromDrawer,
      openAnalyzeFromDrawer,
      openOfficialOnly,
      forceClean,
      openPathSafe,
      drillDownBucket,
      setKindFilter,
    ],
  );

  const softwareProps = useSoftwareController({
    apps,
    filtered,
    loading,
    q,
    category,
    sortCol,
    sortDesc,
    selected,
    multi,
    uninstallingKey,
    formatAppSize,
    sizeOf,
    sortBy,
    selectApp,
    toggleMulti,
    listStartUninstall,
    listAnalyze,
    listForceClean,
    listIgnoreApp,
    listIgnorePublisher,
    estimating,
    scanning,
    aiEnabled,
    uninstallStage,
    showDetail,
    scan,
    selectedPaths,
    evidence,
    setEvidence,
    setSelectedPaths,
    ignoreSuggestions,
    residualFromUninstall,
    useOfficial,
    aiBusy,
    aiRisk,
    setAiRisk,
    aiNotes,
    aiSummaryNote,
    aiNudgeDismissed,
    dryRunning,
    batching,
    error,
    report,
    aiReportBusy,
    aiReportNote,
    setAiReportBusy,
    setAiReportNote,
    verifyRows,
    checkup,
    checkupOpen,
    checkupOrphanCount,
    checkupOrphanBusy,
    batchIndex,
    batchTotal,
    batchCurrent,
    batchResults,
    showBatchSummary,
    setShowBatchSummary,
    retryFailedBatch,
    cancelBatch,
    kindFilter,
    setKindFilter,
    riskFilter,
    setRiskFilter,
    detailPanel,
    core: coreActions,
    residualActions,
    shell: shellCheckup,
    shellActions,
    setCategory,
    stopSizeEstimate,
    goNav,
    dismissAiNudge,
    closePreview,
    dryRun,
    handleCleanupConfirm,
    runAiExplain,
    checkupOrphanScan,
    batchCleanup,
    setQ,
    setCopilotList,
    setCategoryState,
    L,
  });

  return (
    <>
      <style>{globalCss}</style>
      <ConfirmHost />
      <CloseChoiceHost />
      <ToastHost />
      <Shell
        nav={nav}
        onNav={goNav}
        title={navTitle(nav, L)}
        subtitle={navSubtitle(nav, L, apps.length)}
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
        {error && nav !== "software" && (
          <div style={{ flexShrink: 0, overflow: "visible" }}>
            <ErrorBanner error={error} onDismiss={() => core.setError(null)} />
          </div>
        )}
        <Suspense
          fallback={
            <div style={{ padding: 24, color: "var(--muted)", fontSize: 13 }}>
              {L.loadingPage}
            </div>
          }
        >
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
                // lastReport is derived from report by the reducer.
                core.setReport(r);
              }}
              onError={core.setError}
            />
          )}
          {nav === "more" && (
            <MorePage
              selected={selected}
              monitoring={monitoring}
              monitorDiff={monitorDiff}
              lastReport={lastReport}
              closeMode={closeMode}
              onCloseModeChange={setCloseMode}
              onIgnorePublisher={() => void doIgnorePublisher()}
              onToggleMonitor={() => void toggleMonitor()}
              onMonitorToCleanup={() => monitorDiff && void monitorDiffToCleanup(monitorDiff)}
              onDismissMonitor={() => residualActions.setMonitorDiff(null)}
              onExportReport={() => {
                if (lastReport) exportHtmlReport(lastReport, L);
              }}
              onError={core.setError}
              onCheckUpdate={() => void checkUpdateNow(setUpdateInfo, L)}
              onGoSoftware={() => goNav("software")}
              onLastReport={(r) => coreSetReport(r)}
            />
          )}
          {nav === "software" && (
            <SoftwarePage
              {...softwareProps}
            />
          )}
        </Suspense>
      </Shell>
    </>
  );
}
