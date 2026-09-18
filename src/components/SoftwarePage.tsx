import type { ReactNode } from "react";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  IgnoreSuggestion,
  ScanResult,
} from "../types";
import type { SortCol } from "../hooks/useAppFilter";
import type { LinkedBucketId } from "../lib/linkedItems";
import type { UninstallStage } from "./UninstallStageBar";
import type { BatchItemResult } from "./BatchPanels";
import type { ReportVerifyRow } from "./ReportPanel";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { defaultSelectable } from "../lib/decision";
import { SoftwareToolbar, type CategoryId } from "./SoftwareToolbar";
import { UninstallStageBar } from "./UninstallStageBar";
import { SelectedAppCard } from "./SelectedAppCard";
import { IgnoreSuggestBar } from "./IgnoreSuggestBar";
import { ScanActionsBar } from "./ScanActionsBar";
import { ErrorBanner, AiRiskBanner } from "./StatusBanners";
import { ReportPanel } from "./ReportPanel";
import { CheckupPanel } from "./CheckupPanel";
import { BatchProgress, BatchSummaryPanel } from "./BatchPanels";
import { ScanLeftoversView } from "./ScanLeftoversView";
import { CopilotPanel } from "./CopilotPanel";
import { SoftwareListTable } from "./SoftwareListTable";
import { BatchActionBar } from "./BatchActionBar";
import { CleanupConclusion } from "./CleanupConclusion";

export type SoftwarePageProps = {
  // list
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
  setSelected: (a: InstalledApp | null) => void;
  toggleMulti: (key: string) => void;
  listStartUninstall: (a: InstalledApp) => void;
  listAnalyze: (a: InstalledApp) => void;
  listForceClean: (a: InstalledApp) => void;
  listIgnoreApp: (a: InstalledApp) => void;
  listIgnorePublisher: (a: InstalledApp) => void;
  appKey: (a: InstalledApp) => string;
  setMulti: (updater: Set<string> | ((m: Set<string>) => Set<string>)) => void;

  // toolbar / chrome
  estimating: boolean;
  scanning: boolean;
  aiEnabled: boolean;
  uninstallStage: UninstallStage;
  showDetail: boolean;
  onToggleDetail: () => void;
  onQuery: (v: string) => void;
  onCategory: (id: "all" | "desktop" | "store" | "large" | "recent") => void;
  onStopEstimate: () => void;
  onOpenAi: () => void;

  // scan / cleanup
  scan: ScanResult | null;
  selectedPaths: Set<string>;
  evidence: string | null;
  setEvidence: (v: string | null) => void;
  setSelectedPaths: (updater: Set<string> | ((s: Set<string>) => Set<string>)) => void;
  ignoreSuggestions: IgnoreSuggestion[];
  onIgnoreApplied: (pubs: string[], names: string[]) => void;
  onDismissIgnore: () => void;
  residualFromUninstall: boolean;
  useOfficial: boolean;
  setUseOfficial: (v: boolean) => void;
  aiBusy: boolean;
  aiRisk: string | null;
  setAiRisk: (v: string | null) => void;
  aiNotes: Record<string, string>;
  aiSummaryNote: string | null;
  aiNudgeDismissed: boolean;
  onDismissAiNudge: () => void;
  dryRunning: boolean;
  busy: boolean;
  onBack: () => void;
  onDryRun: () => void;
  onCleanup: () => void;
  onAiExplain: () => void;

  // report / checkup / batch
  error: string | null;
  setError: (e: string | null) => void;
  report: CleanupReport | FullCleanupReport | null;
  setReport: (r: CleanupReport | FullCleanupReport | null) => void;
  aiReportBusy: boolean;
  aiReportNote: string | null;
  setAiReportBusy: (v: boolean) => void;
  setAiReportNote: (v: string | null) => void;
  verifyRows: ReportVerifyRow[] | null;
  checkup: { total: number; large: number; recent: number };
  checkupOpen: boolean;
  setCheckupOpen: (v: boolean) => void;
  checkupOrphanCount: number | null;
  checkupOrphanScan: () => void;
  onGoOrphans: () => void;
  batching: boolean;
  batchIndex: number;
  batchTotal: number;
  batchCurrent: string;
  batchResults: BatchItemResult[];
  showBatchSummary: boolean;
  setShowBatchSummary: (v: boolean) => void;
  retryFailedBatch: () => void;
  cancelBatch: () => void;
  batchCleanup: () => void;

  // conclusion / filters / detail
  kindFilter: LinkedBucketId | null;
  setKindFilter: (v: LinkedBucketId | null) => void;
  riskFilter: "confirm" | "keep" | null;
  setRiskFilter: (v: "confirm" | "keep" | null) => void;
  detailPanel: ReactNode;
  onOpenSettings: () => void;
  onCopilotApplyFilter: (list: InstalledApp[]) => void;
  onCopilotBatch: (list: InstalledApp[]) => void;
};

/** Software nav page: toolbar + scan/cleanup + list (extracted from App, FE-02). */
export function SoftwarePage(p: SoftwarePageProps) {
  const L = t();
  return (
    <>
      <SoftwareToolbar
        q={p.q}
        category={p.category}
        estimating={p.estimating}
        scanning={p.scanning}
        aiEnabled={p.aiEnabled}
        onQuery={p.onQuery}
        onCategory={p.onCategory}
        onStopEstimate={p.onStopEstimate}
        onOpenAi={p.onOpenAi}
      />
      {p.uninstallStage !== "idle" && <UninstallStageBar stage={p.uninstallStage} />}

      {p.selected && !p.scan && (
        <SelectedAppCard
          selected={p.selected}
          showDetail={p.showDetail}
          onToggleDetail={p.onToggleDetail}
        />
      )}

      {p.error && <ErrorBanner error={p.error} onDismiss={() => p.setError(null)} />}

      {p.scan && p.ignoreSuggestions.length > 0 && (
        <IgnoreSuggestBar
          suggestions={p.ignoreSuggestions}
          onApplied={p.onIgnoreApplied}
          onDismiss={p.onDismissIgnore}
        />
      )}

      {p.scan && (
        <ScanActionsBar
          scan={p.scan}
          scanning={p.scanning}
          dryRunning={p.dryRunning}
          residualFromUninstall={p.residualFromUninstall}
          useOfficial={p.useOfficial}
          aiEnabled={p.aiEnabled}
          aiBusy={p.aiBusy}
          selectedPaths={p.selectedPaths}
          busy={p.busy}
          onBack={p.onBack}
          onUseOfficial={p.setUseOfficial}
          onDryRun={p.onDryRun}
          onCleanup={p.onCleanup}
          onAiExplain={p.onAiExplain}
        />
      )}

      {p.aiRisk && p.scan && (
        <AiRiskBanner risk={p.aiRisk} onDismiss={() => p.setAiRisk(null)} />
      )}

      {p.report && (
        <ReportPanel
          report={p.report}
          aiEnabled={p.aiEnabled}
          aiReportBusy={p.aiReportBusy}
          aiReportNote={p.aiReportNote}
          verifyRows={p.verifyRows}
          onDismiss={() => p.setReport(null)}
          onAiReportBusy={p.setAiReportBusy}
          onAiReportNote={p.setAiReportNote}
        />
      )}

      {p.checkupOpen && (
        <CheckupPanel
          stats={p.checkup}
          scanning={p.scanning}
          orphanCount={p.checkupOrphanCount}
          onClose={() => p.setCheckupOpen(false)}
          onOrphanScan={p.checkupOrphanScan}
          onOpenOrphans={p.onGoOrphans}
        />
      )}

      {p.batching && p.batchTotal > 0 && (
        <BatchProgress index={p.batchIndex} total={p.batchTotal} current={p.batchCurrent} />
      )}

      {p.showBatchSummary && p.batchResults.length > 0 && (
        <BatchSummaryPanel
          results={p.batchResults}
          onRetryFailed={p.retryFailedBatch}
          onDismiss={() => p.setShowBatchSummary(false)}
        />
      )}

      {p.scan ? (
        <div style={{ display: "flex", gap: 12, flex: 1, minHeight: 0, alignItems: "stretch" }}>
          <ScanLeftoversView
            scan={p.scan}
            scanning={p.scanning}
            selectedPaths={p.selectedPaths}
            evidence={p.evidence}
            aiNotes={p.aiNotes}
            orphanLabel={L.orphanScan}
            kindFilter={p.kindFilter}
            filterApp={p.selected}
            onClearKindFilter={() => p.setKindFilter(null)}
            riskFilter={p.riskFilter}
            onClearRiskFilter={() => p.setRiskFilter(null)}
            conclusion={
              p.scan.app_name !== L.orphanScan && !p.scanning ? (
                <>
                  <CleanupConclusion
                    scan={p.scan}
                    scanning={p.scanning}
                    aiEnabled={p.aiEnabled}
                    aiBusy={p.aiBusy}
                    aiNote={p.aiSummaryNote}
                    onCleanSafe={() => {
                      p.setSelectedPaths(
                        new Set(p.scan!.items.filter(defaultSelectable).map((it) => it.path)),
                      );
                      p.setRiskFilter(null);
                    }}
                    onShowConfirm={() => {
                      p.setKindFilter(null);
                      p.setRiskFilter(p.riskFilter === "confirm" ? null : "confirm");
                    }}
                    onShowKeep={() => {
                      p.setKindFilter(null);
                      p.setRiskFilter(p.riskFilter === "keep" ? null : "keep");
                    }}
                    onExplain={p.onAiExplain}
                    onOpenSettings={p.onOpenSettings}
                  />
                  {!p.aiEnabled && !p.aiNudgeDismissed && p.scan.items.length > 0 && (
                    <div
                      style={{
                        marginBottom: 10,
                        padding: "8px 12px",
                        border: "1px solid var(--border)",
                        borderRadius: 8,
                        background: "var(--surface-2)",
                        display: "flex",
                        gap: 10,
                        alignItems: "center",
                        fontSize: 12,
                        color: "var(--muted)",
                      }}
                    >
                      <span style={{ flex: 1 }}>{L.conclusionEnableAi}</span>
                      <button
                        style={{ ...css.btnSm, height: 28 }}
                        onClick={p.onOpenSettings}
                      >
                        {L.aiSettings}
                      </button>
                      <button
                        style={{ ...css.btnGhost, height: 28 }}
                        onClick={p.onDismissAiNudge}
                      >
                        {L.cancel}
                      </button>
                    </div>
                  )}
                </>
              ) : null
            }
            onTogglePath={(path) =>
              p.setSelectedPaths((s) => {
                const n = new Set(s);
                if (n.has(path)) n.delete(path);
                else n.add(path);
                return n;
              })
            }
            onEvidence={p.setEvidence}
          />
          {p.selected && p.scan.app_name === p.selected.name && p.detailPanel}
        </div>
      ) : (
        <div style={{ display: "flex", gap: 12, flex: 1, minHeight: 0, alignItems: "stretch" }}>
          <div
            style={{
              ...css.card,
              flex: 1,
              minWidth: 0,
              minHeight: 0,
              display: "flex",
              flexDirection: "column",
            }}
          >
            <div style={{ padding: "10px 12px 0", flexShrink: 0 }}>
              <CopilotPanel
                apps={p.apps}
                aiEnabled={p.aiEnabled}
                onApplyFilter={p.onCopilotApplyFilter}
                onAnalyze={(app) => p.listAnalyze(app)}
                onBatch={p.onCopilotBatch}
                onForceClean={(app) => p.listForceClean(app)}
              />
            </div>
            <SoftwareListTable
              filtered={p.filtered}
              loading={p.loading}
              q={p.q}
              category={p.category}
              sortCol={p.sortCol}
              sortDesc={p.sortDesc}
              selected={p.selected}
              multi={p.multi}
              uninstallingKey={p.uninstallingKey}
              appKey={p.appKey}
              sizeText={p.formatAppSize}
              sizeOf={p.sizeOf}
              sortBy={p.sortBy}
              selectApp={p.selectApp}
              startUninstall={p.listStartUninstall}
              analyze={p.listAnalyze}
              forceClean={p.listForceClean}
              doIgnoreApp={p.listIgnoreApp}
              doIgnorePublisher={p.listIgnorePublisher}
              toggleMulti={p.toggleMulti}
              setSelected={p.setSelected}
            />
            {p.multi.size > 0 && !p.scan && (
              <BatchActionBar
                count={p.multi.size}
                batching={p.batching}
                onCancelOrClear={() =>
                  p.batching ? p.cancelBatch() : p.setMulti(new Set<string>())
                }
                onStart={p.batchCleanup}
              />
            )}
          </div>
          {p.detailPanel}
        </div>
      )}
    </>
  );
}
