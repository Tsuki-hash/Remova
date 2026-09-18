import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./lib/api";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "./types";
import { currentLang, loadLang, setLang, t } from "./i18n";
import { cssStyles as css, globalCss } from "./styles";
import { formatError } from "./lib/format";
import { applyTheme, saveNav, type NavId } from "./lib/theme";
import { Shell } from "./components/Shell";
import { ManageListPage } from "./components/ManageListPage";
import { MorePage } from "./components/MorePage";
import { ConfirmHost } from "./components/ui/ConfirmHost";
import { CloseChoiceHost } from "./components/ui/CloseChoiceHost";
import { ToastHost } from "./components/ui/ToastHost";
import { AppDetailPanel } from "./components/AppDetailPanel";
import { OrphanPage } from "./components/OrphanPage";
import { toast } from "./lib/toast";
import { defaultSelectable, isRecentInstall, summarizeLeftovers } from "./lib/decision";
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
import { useListFilterChrome, useResidualState } from "./hooks/useResidualState";
import { SoftwarePage } from "./components/SoftwarePage";
import { ErrorBanner } from "./components/StatusBanners";
import { ShellStatus, ShellFooter } from "./components/ShellChrome";
import { exportHtmlReport } from "./lib/exportHtmlReport";
import { runAiReportSummary } from "./lib/aiNarrative";
import { saveCloseMode, type CloseMode } from "./lib/closeMode";

declare const __APP_VERSION__: string;

function useCheckupStats(apps: InstalledApp[], sizeOf: (a: InstalledApp) => number, sizeMap: Record<string, number>) {
  return useMemo(() => {
    const large = apps.filter((a) => sizeOf(a) > 500 * 1024).length;
    const recent = apps.filter((a) => isRecentInstall(a.install_date, 30)).length;
    return { total: apps.length, large, recent };
  }, [apps, sizeOf, sizeMap]);
}

export default function App() {
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<InstalledApp | null>(null);
  const [multi, setMulti] = useState<Set<string>>(new Set());
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [report, setReport] = useState<CleanupReport | FullCleanupReport | null>(null);
  /** Deep-analyze leftover path can still opt into official uninstaller. */
  const [useOfficial, setUseOfficial] = useState(false);
  const [admin, setAdmin] = useState<boolean | null>(null);
  const [disk, setDisk] = useState("");
  const {
    q,
    setQ,
    sortCol,
    setSortCol,
    sortDesc,
    setSortDesc,
    category,
    setCategoryState,
  } = useListFilterChrome();
  const {
    theme,
    setTheme,
    nav,
    setNav,
    langVer,
    setLangVer,
    shellMenu,
    setShellMenu,
    closeMode,
    setCloseModeState,
    updateInfo,
    setUpdateInfo,
    checkupOpen,
    setCheckupOpen,
    checkupOrphanCount,
    setCheckupOrphanCount,
    uninstallStage,
    setUninstallStage,
    showDetail,
    setShowDetail,
  } = useShellState();
  const setCloseMode = useCallback(
    (m: CloseMode) => {
      setCloseModeState(m);
      saveCloseMode(m);
    },
    [setCloseModeState],
  );
  const [uninstallingKey, setUninstallingKey] = useState<string | null>(null);
  const [ignorePub, setIgnorePub] = useState<string[]>([]);
  const [ignoreName, setIgnoreName] = useState<string[]>([]);
  const {
    selectedPaths,
    setSelectedPaths,
    evidence,
    setEvidence,
    ignoreSuggestions,
    setIgnoreSuggestions,
    lastReport,
    setLastReport,
    monitoring,
    setMonitoring,
    monitorDiff,
    setMonitorDiff,
    residualFromUninstall,
    setResidualFromUninstall,
  } = useResidualState();
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
  } = useAiPanelState();
  const setCategory = useCallback(
    (id: "all" | "desktop" | "store" | "large" | "recent") => {
      setCategoryState(id);
      setCopilotList(null);
      localStorage.setItem("remova_cat", id);
    },
    [setCategoryState, setCopilotList],
  );
  const busyRef = useRef(false);
  /** Linked-bucket drill-down filter for the leftover table. */
  const [kindFilter, setKindFilter] = useState<LinkedBucketId | null>(null);
  const pendingBucketFilter = useRef<LinkedBucketId | null>(null);
  /** Decision-layer list filter: confirm | keep. */
  const [riskFilter, setRiskFilter] = useState<"confirm" | "keep" | null>(null);
  const [aiSummaryNote, setAiSummaryNote] = useState<string | null>(null);
  const [aiNudgeDismissed, setAiNudgeDismissed] = useState(
    () => localStorage.getItem("remova_ai_nudge") === "1",
  );

  const L = useMemo(() => t(), [langVer]);
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
      setApps(list);
    } catch (e) {
      setError(formatError(e));
    }
  }, []);

  const goNav = useCallback((n: NavId) => {
    setNav(n);
    saveNav(n);
  }, []);

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
      setMulti,
      setResidualFromUninstall,
      setAiRisk,
      setReport,
      setLastReport,
      setVerifyRows,
      setAiReportNote,
      setError,
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
    },
    refreshApps,
    busyRef,
  });

  const checkup = useCheckupStats(apps, sizeOf, sizeMap);

  useAppBoot({
    setApps,
    setLoading,
    setError,
    setAdmin,
    setAiEnabled,
    setIgnorePub,
    setIgnoreName,
    setDisk,
    setUpdateInfo,
    busyRef,
  });

  useEffect(() => {
    loadLang();
    applyTheme(theme);
    setLangVer((v) => v + 1);
  }, [theme]);

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

  usePendingAnalyze({ loading, apps, goNav, setSelected, analyze, setQ });
  useDragDropAnalyze({ apps, setSelected, analyze });

  const toggleMulti = (key: string) => {
    setMulti((m) => {
      const n = new Set(m);
      if (n.has(key)) n.delete(key);
      else n.add(key);
      return n;
    });
  };

  const doIgnorePublisher = useCallback(
    async (appOverride?: InstalledApp) => {
      const pub = (appOverride ?? selected)?.publisher;
      if (!pub) return;
      try {
        const ig = await api.ignorePublisher(pub);
        setIgnorePub(ig.publishers || []);
        toast.success(L.ignoreLoaded);
      } catch (e) {
        setError(formatError(e));
      }
    },
    [selected, L],
  );

  const doIgnoreApp = useCallback(
    async (appOverride?: InstalledApp) => {
      const name = (appOverride ?? selected)?.name;
      if (!name) return;
      try {
        const ig = await api.ignoreAppName(name);
        setIgnoreName(ig.names || []);
        toast.success(L.ignoreLoaded);
      } catch (e) {
        setError(formatError(e));
      }
    },
    [selected, L],
  );

  const runOrphanScan = useCallback(async () => {
    toast.info(L.orphanScanning);
    try {
      const items = await api.orphanScan();
      goNav("software");
      setScan({ app_name: L.orphanScan, items });
      // Orphans are unconfirmed by design — do not auto-select.
      setSelectedPaths(new Set());
      if (items.length === 0) toast.info(L.orphanScanEmpty);
      else {
        const s = summarizeLeftovers(items);
        toast.success(L.orphanScanDone(s.total, s.suggest, s.keep));
      }
    } catch (e) {
      setError(formatError(e, "analyze"));
      toast.error(L.errAnalyzeFailed(formatError(e, "analyze")));
    }
  }, [L, goNav]);

  const toggleMonitor = useCallback(async () => {
    try {
      if (!monitoring) {
        await api.beginInstallMonitor();
        setMonitoring(true);
        setMonitorDiff(null);
        toast.info(L.monitorRunning);
      } else {
        const d = await api.endInstallMonitor();
        setMonitoring(false);
        setMonitorDiff(d);
        toast.success(L.toastMonitorDiff(d.added_files.length, d.added_reg_values.length));
      }
    } catch (e) {
      setError(formatError(e));
      toast.error(L.errInvokeFailed(formatError(e)));
      setMonitoring(false);
    }
  }, [monitoring, L]);

  const monitorDiffToCleanup = useCallback(
    async (diff: { added_files: string[]; added_reg_values: string[] }) => {
      try {
        const items = await api.monitorDiffToItems(diff);
        if (!items.length) {
          toast.info(L.monitorNoSnap);
          return;
        }
        goNav("software");
        setScan({ app_name: L.monitorDiff, items });
        setSelectedPaths(new Set(items.filter(defaultSelectable).map((i) => i.path)));
        setMonitorDiff(null);
        toast.success(`${L.monitorToCleanup}: ${items.length}`);
      } catch (e) {
        setError(formatError(e));
      }
    },
    [L, goNav],
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
      for (const o of out) {
        map[o.path] = o.summary;
      }
      setAiNotes(map);
      // Decision-layer one-liner from AI notes (P0).
      const brief = out
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
  }, [scan, selected, aiEnabled, aiBusy, L]);

  // Auto generate AI reading when scan finishes (decision layer, not a button-first flow).
  useEffect(() => {
    setAiSummaryNote(null);
    setRiskFilter(null);
    if (scan && scan.items.length > 0 && aiEnabled) {
      void runAiExplain();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scan?.app_name, scan?.items.length, aiEnabled]);

  const dismissAiNudge = () => {
    setAiNudgeDismissed(true);
    localStorage.setItem("remova_ai_nudge", "1");
  };

  // Auto narrative after cleanup (P2 continuous story).
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
    setScan(null);
    setReport(null);
    setError(null);
    setResidualFromUninstall(false);
    setIgnoreSuggestions([]);
    setKindFilter(null);
    setRiskFilter(null);
    setAiSummaryNote(null);
    pendingBucketFilter.current = null;
    void refreshApps();
  }, [refreshApps]);

  // Apply drill-down filter once a pending analyze finishes.
  useEffect(() => {
    if (scan && pendingBucketFilter.current) {
      setKindFilter(pendingBucketFilter.current);
      pendingBucketFilter.current = null;
    }
    if (!scan) setKindFilter(null);
  }, [scan]);

  const drillDownBucket = useCallback(
    (bucket: LinkedBucketId) => {
      if (scan && scan.app_name === selected?.name) {
        setKindFilter(bucket);
      } else {
        pendingBucketFilter.current = bucket;
        if (selected) void analyze(selected);
      }
    },
    [scan, selected, analyze],
  );

  const checkupOrphanScan = useCallback(() => {
    void (async () => {
      setScanning(true);
      try {
        const items = await api.orphanScan();
        setCheckupOrphanCount(items.length);
      } catch (e) {
        setCheckupOrphanCount(null);
        toast.error(formatError(e, "analyze"));
      } finally {
        setScanning(false);
      }
    })();
  }, []);

  /** Stable row handlers so AppRow.memo is not defeated by inline arrows (PF-01). */
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
    setKindFilter(null);
    setSelected(null);
  }, []);

  /** Single AppDetailPanel instance used in both list and scan layouts (FE-03). */
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

  return (
    <>
      <style>{globalCss}</style>
      <ConfirmHost />
      <CloseChoiceHost />
      <ToastHost />
      <Shell
        nav={nav}
        onNav={goNav}
        title={nav === "software" ? L.navSoftware : nav === "startup" ? L.navStartup : nav === "services" ? L.navServices : nav === "tasks" ? L.navTasks : nav === "orphans" ? L.navOrphans : L.toolboxTitle}
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
            totalCount={nav === "software" ? (loading ? apps.length : filtered.length) : undefined}
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
            <button
              style={css.btnSm}
              onClick={() => setTheme((th) => (th === "dark" ? "light" : "dark"))}
            >
              {L.themeToggle}
            </button>
            <button
              style={css.btnSm}
              onClick={() => {
                const next = currentLang() === "zh" ? "en" : "zh";
                setLang(next);
                setLangVer((v) => v + 1);
              }}
            >
              {L.langToggle}
            </button>
          </>
        }
      >
        {error && <ErrorBanner error={error} onDismiss={() => setError(null)} />}
        {nav === "startup" && (
          <ManageListPage tab="startup" title={L.navStartup} onError={setError} />
        )}
        {nav === "services" && (
          <ManageListPage tab="services" title={L.navServices} onError={setError} />
        )}
        {nav === "tasks" && (
          <ManageListPage tab="tasks" title={L.navTasks} onError={setError} />
        )}
        {nav === "orphans" && (
          <OrphanPage
            onLastReport={(r) => {
              setLastReport(r);
              setReport(r);
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
            onDismissMonitor={() => setMonitorDiff(null)}
            onShellToggle={() => void toggleShellMenuApi(shellMenu, setShellMenu, setError, L)}
            onExportReport={() => {
              if (lastReport) exportHtmlReport(lastReport, L);
            }}
            onError={setError}
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
            setSelected={setSelected}
            toggleMulti={toggleMulti}
            listStartUninstall={listStartUninstall}
            listAnalyze={listAnalyze}
            listForceClean={listForceClean}
            listIgnoreApp={listIgnoreApp}
            listIgnorePublisher={listIgnorePublisher}
            appKey={appKey}
            setMulti={setMulti}
            estimating={estimating}
            scanning={scanning}
            aiEnabled={aiEnabled}
            uninstallStage={uninstallStage}
            showDetail={showDetail}
            onToggleDetail={() => setShowDetail((v) => !v)}
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
              setIgnorePub(pubs);
              setIgnoreName(names);
              setIgnoreSuggestions([]);
            }}
            onDismissIgnore={() => setIgnoreSuggestions([])}
            residualFromUninstall={residualFromUninstall}
            useOfficial={useOfficial}
            setUseOfficial={setUseOfficial}
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
            setError={setError}
            report={report}
            setReport={setReport}
            aiReportBusy={aiReportBusy}
            aiReportNote={aiReportNote}
            setAiReportBusy={setAiReportBusy}
            setAiReportNote={setAiReportNote}
            verifyRows={verifyRows}
            checkup={checkup}
            checkupOpen={checkupOpen}
            setCheckupOpen={setCheckupOpen}
            checkupOrphanCount={checkupOrphanCount}
            checkupOrphanScan={checkupOrphanScan}
            onGoOrphans={() => {
              setCheckupOpen(false);
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
              setMulti(new Set(list.map(appKey)));
              setCopilotList(list);
              toast.info(L.batchUninstall);
            }}
          />
        )}
      </Shell>
    </>
  );
}
