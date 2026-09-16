import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./lib/api";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "./types";
import { currentLang, loadLang, setLang, t } from "./i18n";
import { type UpdateInfo } from "./lib/updateCheck";
import { cssStyles as css, globalCss } from "./styles";
import {
  BatchProgress,
  BatchSummaryPanel,
} from "./components/BatchPanels";
import { formatError } from "./lib/format";
import { applyTheme, loadNav, loadTheme, saveNav, type NavId, type Theme } from "./lib/theme";
import { Shell } from "./components/Shell";
import { ManageListPage } from "./components/ManageListPage";
import { MorePage } from "./components/MorePage";
import { ConfirmHost } from "./components/ui/ConfirmHost";
import { CloseChoiceHost } from "./components/ui/CloseChoiceHost";
import { ToastHost } from "./components/ui/ToastHost";
import { AppDetailPanel } from "./components/AppDetailPanel";
import { UninstallStageBar, type UninstallStage } from "./components/UninstallStageBar";
import { CheckupPanel } from "./components/CheckupPanel";
import { ReportPanel } from "./components/ReportPanel";
import { IgnoreSuggestBar } from "./components/IgnoreSuggestBar";
import { ScanLeftoversView } from "./components/ScanLeftoversView";
import { SoftwareToolbar } from "./components/SoftwareToolbar";
import { OrphanPage } from "./components/OrphanPage";
import { toast } from "./lib/toast";
import { defaultSelectable, summarizeLeftovers } from "./lib/decision";
import type { IgnoreSuggestion } from "./types";
import { appKey } from "./lib/appKey";
import { useSizeEstimate } from "./hooks/useSizeEstimate";
import { useAppFilter } from "./hooks/useAppFilter";
import { usePendingAnalyze, useDragDropAnalyze } from "./hooks/useAppNavAssist";
import { useCleanupHandlers } from "./hooks/useCleanupHandlers";
import { useAnalyzeFlow } from "./hooks/useAnalyzeFlow";
import { useAppBoot, checkUpdateNow, toggleShellMenuApi } from "./hooks/useAppBoot";
import { SoftwareListTable } from "./components/SoftwareListTable";
import { ScanActionsBar } from "./components/ScanActionsBar";
import { SelectedAppCard } from "./components/SelectedAppCard";
import { ErrorBanner, AiRiskBanner } from "./components/StatusBanners";
import { ShellStatus, ShellFooter } from "./components/ShellChrome";
import { BatchActionBar } from "./components/BatchActionBar";
import { exportHtmlReport } from "./lib/exportHtmlReport";
import { loadCloseMode, saveCloseMode, type CloseMode } from "./lib/closeMode";

declare const __APP_VERSION__: string;

function useCheckupStats(apps: InstalledApp[], sizeOf: (a: InstalledApp) => number, sizeMap: Record<string, number>) {
  return useMemo(() => {
    const large = apps.filter((a) => sizeOf(a) > 500 * 1024).length;
    const recent = apps.filter((a) => {
      const d = (a.install_date || "").trim();
      if (!d) return false;
      const ymd = d.match(/^(\d{4})[-/.]?(\d{2})[-/.]?(\d{2})$/);
      if (!ymd) return false;
      const dt = new Date(Number(ymd[1]), Number(ymd[2]) - 1, Number(ymd[3]));
      if (Number.isNaN(dt.getTime())) return false;
      return Date.now() - dt.getTime() <= 30 * 24 * 60 * 60 * 1000;
    }).length;
    return { total: apps.length, large, recent };
  }, [apps, sizeOf, sizeMap]);
}

export default function App() {
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<InstalledApp | null>(null);
  const [multi, setMulti] = useState<Set<string>>(new Set());
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [report, setReport] = useState<CleanupReport | FullCleanupReport | null>(null);
  /** Deep-analyze leftover path can still opt into official uninstaller. */
  const [useOfficial, setUseOfficial] = useState(false);
  const [sortCol, setSortCol] = useState<"name" | "size" | "recommend" | null>(null);
  const [sortDesc, setSortDesc] = useState(false);
  const [admin, setAdmin] = useState<boolean | null>(null);
  const [disk, setDisk] = useState("");
  const [theme, setTheme] = useState<Theme>(loadTheme());
  const [nav, setNav] = useState<NavId>(loadNav());
  const [category, setCategoryState] = useState<"all" | "desktop" | "store" | "large" | "recent">(
    () => {
      const v = localStorage.getItem("remova_cat");
      return v === "desktop" || v === "store" || v === "large" || v === "recent" ? v : "all";
    },
  );
  const setCategory = useCallback((id: "all" | "desktop" | "store" | "large" | "recent") => {
    setCategoryState(id);
    setCopilotList(null);
    localStorage.setItem("remova_cat", id);
  }, []);
  const [langVer, setLangVer] = useState(0);
  const [shellMenu, setShellMenu] = useState(false);
  const [closeMode, setCloseModeState] = useState<CloseMode | null>(() => loadCloseMode());
  const setCloseMode = useCallback((m: CloseMode) => {
    setCloseModeState(m);
    saveCloseMode(m);
  }, []);
  const [uninstallingKey, setUninstallingKey] = useState<string | null>(null);
  /** Residual cleanup after official uninstall always skips a second official run. */
  const [residualFromUninstall, setResidualFromUninstall] = useState(false);
  const [ignorePub, setIgnorePub] = useState<string[]>([]);
  const [ignoreName, setIgnoreName] = useState<string[]>([]);
  const [monitoring, setMonitoring] = useState(false);
  const [monitorDiff, setMonitorDiff] = useState<{
    added_files: string[];
    added_reg_values: string[];
  } | null>(null);
  const [lastReport, setLastReport] = useState<FullCleanupReport | null>(null);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [evidence, setEvidence] = useState<string | null>(null);
  const [showDetail, setShowDetail] = useState(true);
  const [uninstallStage, setUninstallStage] = useState<UninstallStage>("idle");
  const [checkupOpen, setCheckupOpen] = useState(false);
  const [checkupOrphanCount, setCheckupOrphanCount] = useState<number | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [aiEnabled, setAiEnabled] = useState(false);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiNotes, setAiNotes] = useState<Record<string, string>>({});
  const [aiRisk, setAiRisk] = useState<string | null>(null);
  const [aiReportNote, setAiReportNote] = useState<string | null>(null);
  const [verifyRows, setVerifyRows] = useState<
    { path: string; kind: string; still_there: boolean }[] | null
  >(null);
  const [aiReportBusy, setAiReportBusy] = useState(false);
  const [copilotList, setCopilotList] = useState<InstalledApp[] | null>(null);
  const [ignoreSuggestions, setIgnoreSuggestions] = useState<IgnoreSuggestion[]>([]);
  const busyRef = useRef(false);

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
    apps,
    multi,
    setMulti,
    setResidualFromUninstall,
    setAiRisk,
    setReport,
    setLastReport,
    setVerifyRows,
    setAiReportNote,
    refreshApps,
    setError,
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
        kind: it.kind,
        confidence: it.confidence,
        risk: it.risk,
        reason: it.reason,
        evidence_labels: (it.evidence || []).map((e) => e.label).filter(Boolean),
      }));
      const out = await api.aiExplain(scan.app_name, selected?.publisher || "", items);
      const map: Record<string, string> = {};
      for (const o of out) {
        map[o.path] = o.summary;
      }
      setAiNotes(map);
      if (!out.length) toast.info(L.aiDisabledHint);
    } catch {
      toast.error(L.aiFailed);
    } finally {
      setAiBusy(false);
    }
  }, [scan, selected, aiEnabled, aiBusy, L]);

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
    void refreshApps();
  }, [refreshApps]);

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
          />
        )}
        {nav === "software" && (
          <>
            <SoftwareToolbar
              q={q}
              category={category}
              estimating={estimating}
              scanning={scanning}
              onQuery={(v) => {
                setQ(v);
                setCopilotList(null);
              }}
              onCategory={setCategory}
              onStopEstimate={() => void stopSizeEstimate()}
            />
            {uninstallStage !== "idle" && <UninstallStageBar stage={uninstallStage} />}

            {selected && !scan && (
              <SelectedAppCard
                selected={selected}
                showDetail={showDetail}
                onToggleDetail={() => setShowDetail((v) => !v)}
              />
            )}

            {scan && ignoreSuggestions.length > 0 && (
              <IgnoreSuggestBar
                suggestions={ignoreSuggestions}
                onApplied={(p, n) => {
                  setIgnorePub(p);
                  setIgnoreName(n);
                  setIgnoreSuggestions([]);
                }}
                onDismiss={() => setIgnoreSuggestions([])}
              />
            )}

            {scan && (
              <ScanActionsBar
                scan={scan}
                scanning={scanning}
                dryRunning={dryRunning}
                residualFromUninstall={residualFromUninstall}
                useOfficial={useOfficial}
                aiEnabled={aiEnabled}
                aiBusy={aiBusy}
                selectedPaths={selectedPaths}
                busy={busyRef.current}
                onBack={closePreview}
                onUseOfficial={setUseOfficial}
                onDryRun={() => void dryRun()}
                onCleanup={() => void handleCleanupConfirm()}
                onAiExplain={() => void runAiExplain()}
              />
            )}

            {aiRisk && scan && (
              <AiRiskBanner risk={aiRisk} onDismiss={() => setAiRisk(null)} />
            )}

            {report && (
              <ReportPanel
                report={report}
                aiEnabled={aiEnabled}
                aiReportBusy={aiReportBusy}
                aiReportNote={aiReportNote}
                verifyRows={verifyRows}
                onDismiss={() => setReport(null)}
                onAiReportBusy={setAiReportBusy}
                onAiReportNote={setAiReportNote}
              />
            )}

            {checkupOpen && (
              <CheckupPanel
                stats={checkup}
                scanning={scanning}
                orphanCount={checkupOrphanCount}
                onClose={() => setCheckupOpen(false)}
                onOrphanScan={checkupOrphanScan}
                onOpenOrphans={() => {
                  setCheckupOpen(false);
                  goNav("orphans");
                }}
              />
            )}

            {batching && batchTotal > 0 && (
              <BatchProgress index={batchIndex} total={batchTotal} current={batchCurrent} />
            )}

            {showBatchSummary && batchResults.length > 0 && (
              <BatchSummaryPanel
                results={batchResults}
                onRetryFailed={retryFailedBatch}
                onDismiss={() => setShowBatchSummary(false)}
              />
            )}

            {scan ? (
              <ScanLeftoversView
                scan={scan}
                scanning={scanning}
                selectedPaths={selectedPaths}
                evidence={evidence}
                aiNotes={aiNotes}
                orphanLabel={L.orphanScan}
                onTogglePath={(path) =>
                  setSelectedPaths((s) => {
                    const n = new Set(s);
                    if (n.has(path)) n.delete(path);
                    else n.add(path);
                    return n;
                  })
                }
                onEvidence={setEvidence}
              />
            ) : (
              <div style={{ display: "flex", gap: 12, flex: 1, minHeight: 0, alignItems: "stretch" }}>
                <div style={{ ...css.card, flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
                  <SoftwareListTable
                    filtered={filtered}
                    loading={loading}
                    q={q}
                    category={category}
                    sortCol={sortCol}
                    sortDesc={sortDesc}
                    selected={selected}
                    multi={multi}
                    uninstallingKey={uninstallingKey}
                    appKey={appKey}
                    sizeText={formatAppSize}
                    sizeOf={sizeOf}
                    sortBy={sortBy}
                    selectApp={selectApp}
                    startUninstall={(a) => void startUninstall(a)}
                    analyze={(a) => void analyze(a)}
                    forceClean={(a) => void forceClean(a)}
                    doIgnoreApp={(a) => void doIgnoreApp(a)}
                    doIgnorePublisher={(a) => void doIgnorePublisher(a)}
                    toggleMulti={toggleMulti}
                    setSelected={setSelected}
                  />
                  {multi.size > 0 && !scan && (
                    <BatchActionBar
                      count={multi.size}
                      batching={batching}
                      onCancelOrClear={() => (batching ? cancelBatch() : setMulti(new Set()))}
                      onStart={() => void batchCleanup()}
                    />
                  )}
                </div>
                {selected && !scan && (
                  <AppDetailPanel
                    app={selected}
                    sizeText={formatAppSize(selected)}
                    sizeKb={sizeOf(selected)}
                    uninstalling={uninstallingKey === appKey(selected)}
                    scan={scan}
                    onClose={() => setSelected(null)}
                    onDeepUninstall={openDeepFromDrawer}
                    onAnalyze={openAnalyzeFromDrawer}
                    onOfficialOnly={(app) => void openOfficialOnly(app)}
                    onForceClean={(app) => void forceClean(app)}
                    onOpenPath={async (path) => {
                      try {
                        await api.openPath(path);
                      } catch (e) {
                        toast.error(formatError(e));
                      }
                    }}
                  />
                )}
              </div>
            )}
          </>
        )}
      </Shell>
    </>
  );
}
