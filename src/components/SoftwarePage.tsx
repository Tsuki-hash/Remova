import type { ReactNode } from "react";
import { memo, useState } from "react";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  IgnoreSuggestion,
  ScanResult,
} from "../types";
import type { CategoryId, SortCol } from "../lib/categories";
import type { LinkedBucketId } from "../lib/linkedItems";
import { scanMatchesApp } from "../lib/appKey";
import type { UninstallStage } from "./UninstallStageBar";
import type { BatchItemResult } from "./BatchPanels";
import type { VerifyRow } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { defaultSelectable } from "../lib/decision";
import { SoftwareToolbar } from "./SoftwareToolbar";
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
  dryRunning: boolean;
  busy: boolean;
  onBack: () => void;
  onDryRun: () => void;
  onCleanup: () => void;
  onAiExplain: () => void;
  onRegenerateAiReport: () => void;

 // report / checkup / batch
  error: string | null;
  setError: (e: string | null) => void;
  report: CleanupReport | FullCleanupReport | null;
  setReport: (r: CleanupReport | FullCleanupReport | null) => void;
  aiReportBusy: boolean;
  aiReportNote: string | null;
  setAiReportBusy: (v: boolean) => void;
  setAiReportNote: (v: string | null) => void;
  verifyRows: VerifyRow[] | null;
  checkup: { total: number; large: number; recent: number };
  checkupOpen: boolean;
  setCheckupOpen: (v: boolean) => void;
  checkupOrphanCount: number | null;
  checkupOrphanBusy: boolean;
  checkupScanStatus?: ReactNode;
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

/** Software nav page: toolbar + scan/cleanup + list (extracted from App). */
export const SoftwarePage = memo(function SoftwarePage(p: SoftwarePageProps) {
  const L = t();
  const [smartFilterOpen, setSmartFilterOpen] = useState(false);
  return (
    <>
      <SoftwareToolbar
        q={p.q}
        category={p.category}
        estimating={p.estimating}
        scanning={p.scanning}
        aiEnabled={p.aiEnabled}
        smartFilterOpen={smartFilterOpen}
        onQuery={p.onQuery}
        onCategory={p.onCategory}
        onStopEstimate={p.onStopEstimate}
        onOpenAi={p.onOpenAi}
        onToggleSmartFilter={() => setSmartFilterOpen((v) => !v)}
      />
      {p.uninstallStage !== "idle" && <UninstallStageBar stage={p.uninstallStage} />}
      {p.residualFromUninstall && !p.report && <div role="status" style={{ ...css.card, padding: 12, marginBottom: 10, lineHeight: 1.6 }}>
        <strong>{L.uninstallOk}</strong>
        <div style={{ color: "var(--muted)" }}>{p.scanning ? L.analyzing : p.scan
          ? p.scan.items.length === 0 ? L.cleanupRescanResult(0) : L.uninstallAwaitCleanup(p.scan.items.length)
          : L.cleanupRescanUnknown}</div>
      </div>}

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
          canRunOfficial={p.selected?.source !== "Monitor" && p.selected?.source !== "Orphan"}
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
          onOpenSettings={p.onOpenSettings}
          aiNote={p.aiSummaryNote}
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
          residualScan={{ scanning: p.scanning, count: p.residualFromUninstall ? p.scan?.items.length : undefined }}
          onDismiss={() => p.setReport(null)}
          onRegenerate={p.onRegenerateAiReport}
        />
      )}

      {p.checkupOpen && (
        <CheckupPanel
          stats={p.checkup}
          orphanScanning={p.checkupOrphanBusy}
          scanStatus={p.checkupScanStatus}
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
            onSelectSafe={() => {
              p.setSelectedPaths(
                new Set(p.scan!.items.filter(defaultSelectable).map((it) => it.path)),
              );
              p.setKindFilter(null);
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
            onTogglePath={(path) =>
              p.setSelectedPaths((s) => {
                const n = new Set(s);
                if (n.has(path)) n.delete(path);
                else n.add(path);
                return n;
              })
            }
            onEvidence={p.setEvidence}
            onSelectVisible={(paths, selected) => p.setSelectedPaths(previous => {
              const next = new Set(previous);
              for (const path of paths) { if (selected) next.add(path); else next.delete(path); }
              return next;
            })}
          />
          {p.selected && p.scan && scanMatchesApp(p.scan, p.selected) && p.detailPanel}
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
              {smartFilterOpen && (
                <CopilotPanel
                  apps={p.apps}
                  aiEnabled={p.aiEnabled}
                  onApplyFilter={p.onCopilotApplyFilter}
                  onAnalyze={(app) => p.listAnalyze(app)}
                  onBatch={p.onCopilotBatch}
                  onForceClean={(app) => p.listForceClean(app)}
                />
              )}
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
          {p.selected ? p.detailPanel : null}
        </div>
      )}
    </>
  );
});
