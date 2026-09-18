import { useMemo } from "react";
import type { ReactNode } from "react";
import type {
  CleanupReport,
  FullCleanupReport,
  IgnoreSuggestion,
  InstalledApp,
  ScanResult,
} from "../types";
import type { CategoryId, SortCol } from "../lib/categories";
import type { LinkedBucketId } from "../lib/linkedItems";
import type { UninstallStage } from "../components/UninstallStageBar";
import type { BatchItemResult } from "../components/BatchPanels";
import type { ReportVerifyRow } from "../components/ReportPanel";
import type { SoftwarePageProps } from "../components/SoftwarePage";
import type { Strings } from "../i18n";
import { appKey } from "../lib/appKey";
import { toast } from "../lib/toast";

/** Domain slices App passes into the software controller (A-04). */
export type SoftwareControllerInput = {
  apps: InstalledApp[];
  filtered: InstalledApp[];
  loading: boolean;
  q: string;
  category: CategoryId;
  sortCol: SortCol;
  sortDesc: boolean;
  selected: InstalledApp | null;
  multi: Set<string>;
  uninstallingKey: string | null;
  formatAppSize: (a: InstalledApp) => string;
  sizeOf: (a: InstalledApp) => number;
  sortBy: (col: "name" | "size" | "recommend") => void;
  selectApp: (a: InstalledApp) => void;
  toggleMulti: (key: string) => void;
  listStartUninstall: (a: InstalledApp) => void;
  listAnalyze: (a: InstalledApp) => void;
  listForceClean: (a: InstalledApp) => void;
  listIgnoreApp: (a: InstalledApp) => void;
  listIgnorePublisher: (a: InstalledApp) => void;
  estimating: boolean;
  scanning: boolean;
  aiEnabled: boolean;
  uninstallStage: UninstallStage;
  showDetail: boolean;
  scan: ScanResult | null;
  selectedPaths: Set<string>;
  evidence: string | null;
  setEvidence: (v: string | null) => void;
  setSelectedPaths: SoftwarePageProps["setSelectedPaths"];
  ignoreSuggestions: IgnoreSuggestion[];
  residualFromUninstall: boolean;
  useOfficial: boolean;
  aiBusy: boolean;
  aiRisk: string | null;
  setAiRisk: (v: string | null) => void;
  aiNotes: Record<string, string>;
  aiSummaryNote: string | null;
  aiNudgeDismissed: boolean;
  dryRunning: boolean;
  batching: boolean;
  error: string | null;
  report: CleanupReport | FullCleanupReport | null;
  aiReportBusy: boolean;
  aiReportNote: string | null;
  setAiReportBusy: (v: boolean) => void;
  setAiReportNote: (v: string | null) => void;
  verifyRows: ReportVerifyRow[] | null;
  checkup: { total: number; large: number; recent: number };
  checkupOpen: boolean;
  checkupOrphanCount: number | null;
  batchIndex: number;
  batchTotal: number;
  batchCurrent: string;
  batchResults: BatchItemResult[];
  showBatchSummary: boolean;
  setShowBatchSummary: (v: boolean) => void;
  retryFailedBatch: () => void;
  cancelBatch: () => void;
  kindFilter: LinkedBucketId | null;
  setKindFilter: (v: LinkedBucketId | null) => void;
  riskFilter: "confirm" | "keep" | null;
  setRiskFilter: (v: "confirm" | "keep" | null) => void;
  detailPanel: ReactNode;
  // domain action bags
  core: {
    setSelected: (a: InstalledApp | null) => void;
    setMulti: SoftwarePageProps["setMulti"];
    setIgnorePub: (v: string[]) => void;
    setIgnoreName: (v: string[]) => void;
    setUseOfficial: (v: boolean) => void;
    setError: (e: string | null) => void;
    setReport: (r: CleanupReport | FullCleanupReport | null) => void;
  };
  residualActions: {
    clearIgnoreSuggestions: () => void;
  };
  shell: {
    setCheckupOpen: (v: boolean) => void;
  };
  shellActions: {
    toggleDetail: () => void;
    closeCheckup: () => void;
  };
  setCategory: (id: CategoryId) => void;
  stopSizeEstimate: () => Promise<void>;
  goNav: (n: "more" | "orphans") => void;
  dismissAiNudge: () => void;
  closePreview: () => void;
  dryRun: () => Promise<void>;
  handleCleanupConfirm: () => Promise<void> | void;
  runAiExplain: () => Promise<void> | void;
  checkupOrphanScan: () => void;
  batchCleanup: () => Promise<void>;
  setQ: (v: string) => void;
  setCopilotList: (l: InstalledApp[] | null) => void;
  setCategoryState: (id: CategoryId) => void;
  L: Strings;
};

/** Memoized SoftwarePage props (A-04 / P-02) — single controller for React.memo. */
export function useSoftwareController(input: SoftwareControllerInput): SoftwarePageProps {
  const {
    core,
    residualActions,
    shell,
    shellActions,
    L,
  } = input;

  return useMemo(
    () =>
      ({
        apps: input.apps,
        filtered: input.filtered,
        loading: input.loading,
        q: input.q,
        category: input.category,
        sortCol: input.sortCol,
        sortDesc: input.sortDesc,
        selected: input.selected,
        multi: input.multi,
        uninstallingKey: input.uninstallingKey,
        formatAppSize: input.formatAppSize,
        sizeOf: input.sizeOf,
        sortBy: input.sortBy,
        selectApp: input.selectApp,
        setSelected: core.setSelected,
        toggleMulti: input.toggleMulti,
        listStartUninstall: input.listStartUninstall,
        listAnalyze: input.listAnalyze,
        listForceClean: input.listForceClean,
        listIgnoreApp: input.listIgnoreApp,
        listIgnorePublisher: input.listIgnorePublisher,
        appKey,
        setMulti: core.setMulti,
        estimating: input.estimating,
        scanning: input.scanning,
        aiEnabled: input.aiEnabled,
        uninstallStage: input.uninstallStage,
        showDetail: input.showDetail,
        onToggleDetail: shellActions.toggleDetail,
        onQuery: (v: string) => {
          input.setQ(v);
          input.setCopilotList(null);
        },
        onCategory: input.setCategory,
        onStopEstimate: () => void input.stopSizeEstimate(),
        onOpenAi: () => input.goNav("more"),
        scan: input.scan,
        selectedPaths: input.selectedPaths,
        evidence: input.evidence,
        setEvidence: input.setEvidence,
        setSelectedPaths: input.setSelectedPaths,
        ignoreSuggestions: input.ignoreSuggestions,
        onIgnoreApplied: (pubs: string[], names: string[]) => {
          core.setIgnorePub(pubs);
          core.setIgnoreName(names);
          residualActions.clearIgnoreSuggestions();
        },
        onDismissIgnore: () => residualActions.clearIgnoreSuggestions(),
        residualFromUninstall: input.residualFromUninstall,
        useOfficial: input.useOfficial,
        setUseOfficial: core.setUseOfficial,
        aiBusy: input.aiBusy,
        aiRisk: input.aiRisk,
        setAiRisk: input.setAiRisk,
        aiNotes: input.aiNotes,
        aiSummaryNote: input.aiSummaryNote,
        aiNudgeDismissed: input.aiNudgeDismissed,
        onDismissAiNudge: input.dismissAiNudge,
        dryRunning: input.dryRunning,
        busy: input.dryRunning || input.batching || input.scanning || input.aiBusy,
        onBack: input.closePreview,
        onDryRun: () => void input.dryRun(),
        onCleanup: () => void input.handleCleanupConfirm(),
        onAiExplain: () => void input.runAiExplain(),
        error: input.error,
        setError: core.setError,
        report: input.report,
        setReport: core.setReport,
        aiReportBusy: input.aiReportBusy,
        aiReportNote: input.aiReportNote,
        setAiReportBusy: input.setAiReportBusy,
        setAiReportNote: input.setAiReportNote,
        verifyRows: input.verifyRows as ReportVerifyRow[] | null,
        checkup: input.checkup,
        checkupOpen: input.checkupOpen,
        setCheckupOpen: shell.setCheckupOpen,
        checkupOrphanCount: input.checkupOrphanCount,
        checkupOrphanScan: input.checkupOrphanScan,
        onGoOrphans: () => {
          shellActions.closeCheckup();
          input.goNav("orphans");
        },
        batching: input.batching,
        batchIndex: input.batchIndex,
        batchTotal: input.batchTotal,
        batchCurrent: input.batchCurrent,
        batchResults: input.batchResults,
        showBatchSummary: input.showBatchSummary,
        setShowBatchSummary: input.setShowBatchSummary,
        retryFailedBatch: input.retryFailedBatch,
        cancelBatch: input.cancelBatch,
        batchCleanup: () => void input.batchCleanup(),
        kindFilter: input.kindFilter,
        setKindFilter: input.setKindFilter,
        riskFilter: input.riskFilter,
        setRiskFilter: input.setRiskFilter,
        detailPanel: input.detailPanel,
        onOpenSettings: () => input.goNav("more"),
        onCopilotApplyFilter: (list: InstalledApp[]) => {
          input.setCopilotList(list);
          input.setQ("");
          input.setCategoryState("all");
        },
        onCopilotBatch: (list: InstalledApp[]) => {
          core.setMulti(new Set(list.map(appKey)));
          input.setCopilotList(list);
          toast.info(L.batchUninstall);
        },
      }) satisfies SoftwarePageProps,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      input.apps,
      input.filtered,
      input.loading,
      input.q,
      input.category,
      input.sortCol,
      input.sortDesc,
      input.selected,
      input.multi,
      input.uninstallingKey,
      input.formatAppSize,
      input.sizeOf,
      input.sortBy,
      input.selectApp,
      input.toggleMulti,
      input.listStartUninstall,
      input.listAnalyze,
      input.listForceClean,
      input.listIgnoreApp,
      input.listIgnorePublisher,
      input.estimating,
      input.scanning,
      input.aiEnabled,
      input.uninstallStage,
      input.showDetail,
      input.scan,
      input.selectedPaths,
      input.evidence,
      input.setEvidence,
      input.setSelectedPaths,
      input.ignoreSuggestions,
      input.residualFromUninstall,
      input.useOfficial,
      input.aiBusy,
      input.aiRisk,
      input.setAiRisk,
      input.aiNotes,
      input.aiSummaryNote,
      input.aiNudgeDismissed,
      input.dryRunning,
      input.batching,
      input.error,
      input.report,
      input.aiReportBusy,
      input.aiReportNote,
      input.setAiReportBusy,
      input.setAiReportNote,
      input.verifyRows,
      input.checkup,
      input.checkupOpen,
      input.checkupOrphanCount,
      input.batchIndex,
      input.batchTotal,
      input.batchCurrent,
      input.batchResults,
      input.showBatchSummary,
      input.setShowBatchSummary,
      input.retryFailedBatch,
      input.cancelBatch,
      input.kindFilter,
      input.setKindFilter,
      input.riskFilter,
      input.setRiskFilter,
      input.detailPanel,
      core,
      residualActions,
      shell,
      shellActions,
      input.setCategory,
      input.stopSizeEstimate,
      input.goNav,
      input.dismissAiNudge,
      input.closePreview,
      input.dryRun,
      input.handleCleanupConfirm,
      input.runAiExplain,
      input.checkupOrphanScan,
      input.batchCleanup,
      input.setQ,
      input.setCopilotList,
      input.setCategoryState,
      L,
    ],
  );
}

export type { SoftwarePageProps };
