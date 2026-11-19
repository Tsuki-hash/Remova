import { cleanupItemKey } from "../lib/cleanupPlan";
import type { ReactNode } from "react";
import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  IgnoreSuggestion,
  ScanResult,
} from "../types";
import type { CategoryId, SortCol } from "../lib/categories";
import type { LinkedBucketId } from "../lib/linkedItems";
import { findReportApp, scanMatchesApp } from "../lib/appKey";
import { shouldRescanAfterUninstall } from "../lib/cleanupFeedback";
import { reviewSessionFor } from "../lib/rescanRounds";
import { canRescanReview, sameReviewApp, uninstallRescanText, type UninstallReview } from "../lib/uninstallReview";
import { prettyAppName } from "../lib/format";
import type { AnalyzeOptions } from "../hooks/useAnalyzeFlow";
import type { UninstallStage } from "./UninstallStageBar";
import type { BatchItemResult } from "./BatchPanels";
import type { VerifyRow } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { defaultSelectable } from "../lib/decision";
import { degradedText } from "../lib/degradedText";
import { SoftwareToolbar } from "./SoftwareToolbar";
import { UninstallStageBar } from "./UninstallStageBar";
import { PendingUninstallReviews } from "./PendingUninstallReviews";
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

/** Report identity -> the InstalledApp it was opened for. Module-level so the
 * mapping survives navigating away from the software page and back, which is
 * exactly when a report outlives its app's presence in the refreshed list.
 * Bounded and time-limited: a long session cannot retain every report's app,
 * and a stale snapshot cannot answer for a much newer report. */
const REPORT_APP_CACHE_MAX = 64;
const REPORT_APP_CACHE_TTL_MS = 24 * 60 * 60 * 1000;
const reportAppCache = new Map<string, { app: InstalledApp; at: number }>();

/** Test hook: the cache is process-level, so suites must reset it between cases. */
export function clearReportAppCache() {
  reportAppCache.clear();
}

function cachedReportApp(identity: string): InstalledApp | undefined {
  const entry = reportAppCache.get(identity);
  if (!entry) return undefined;
  if (Date.now() - entry.at > REPORT_APP_CACHE_TTL_MS) {
    reportAppCache.delete(identity);
    return undefined;
  }
  return entry.app;
}

function cacheReportApp(identity: string, app: InstalledApp) {
  reportAppCache.delete(identity);
  reportAppCache.set(identity, { app, at: Date.now() });
  while (reportAppCache.size > REPORT_APP_CACHE_MAX) {
    const oldest = reportAppCache.keys().next().value;
    if (oldest === undefined) break;
    reportAppCache.delete(oldest);
  }
}

export type SoftwarePageProps = {
 // list
  apps: InstalledApp[];
  filtered: InstalledApp[];
  loading: boolean;
  inventoryIncomplete?: boolean;
  refreshInventory?: () => Promise<void>;
  q: string;
  category: CategoryId;
  sortCol: SortCol;
  sortDesc: boolean;
  selected: InstalledApp | null;
  scanTarget?: InstalledApp | null;
  multi: Set<string>;
  uninstallingKey: string | null;
  formatAppSize: (a: InstalledApp) => string;
  sizeOf: (a: InstalledApp) => number;
  sortBy: (col: "name" | "size" | "recommend") => void;
  selectApp: (a: InstalledApp) => void;
  setSelected: (a: InstalledApp | null) => void;
  toggleMulti: (key: string) => void;
  listStartUninstall: (a: InstalledApp) => void;
  listAnalyze: (
    a: InstalledApp,
    opts?: AnalyzeOptions,
  ) => void;
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
  uninstallReviews: readonly UninstallReview[];
  scanReviewReceipt?: number;
  useOfficial: boolean;
  setUseOfficial: (v: boolean) => void;
  aiBusy: boolean;
  aiRisk: string | null;
  setAiRisk: (v: string | null) => void;
  aiNotes: Record<string, string>;
  aiSummaryNote: string | null;
  dryRunning: boolean;
  previewStale?: boolean;
  busy: boolean;
  onBack: () => void;
  onDryRun: () => void;
  onCleanup: () => void;
  onCancelCleanup?: () => void;
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
  checkupOrphanCompleteness: import("../types").ScanCompleteness | null;
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
  const [inventoryRefreshing, setInventoryRefreshing] = useState(false);
  const inventoryRef = useRef<HTMLDivElement>(null);
  const softwareSearch = useRef<HTMLInputElement>(null);
  const lastListFocus = useRef<HTMLElement | null>(null);
  const hasInventoryDetail = !p.scan && Boolean(p.selected && p.detailPanel);
  useLayoutEffect(() => {
    if (hasInventoryDetail && window.matchMedia?.("(max-width: 1020px)").matches) {
      inventoryRef.current?.querySelector<HTMLButtonElement>(".software-detail-back")?.focus();
    } else if (!hasInventoryDetail && lastListFocus.current) {
      if (lastListFocus.current.isConnected) lastListFocus.current.focus({ preventScroll: true });
      lastListFocus.current = null;
    }
  }, [hasInventoryDetail]);
  const report = p.report;
  const liveReportApp = report ? findReportApp(report, p.apps, p.selected) : undefined;
  // A successful uninstall removes the app from the refreshed list and the
  // reducer clears `selected`. Keep the last resolved app for this exact
  // report so rescan/deep-check/lock-root keep working; never let a different
  // report inherit it. The cache is module-level because this page unmounts
  // when the user navigates to another nav entry — a component-local ref was
  // rebuilt empty on return, which dropped rescan and lock actions for a
  // report whose app had already left the list.
  const reportIdentity = report ? `${report.app_name}\u0000${report.app_key ?? ""}` : "";
  const reportApp =
    liveReportApp ??
    (report ? cachedReportApp(reportIdentity) : undefined);
  const reportReceipt = report && "uninstall_review" in report ? report.uninstall_review : undefined;
  const reportReview = reportReceipt === undefined ? undefined : p.uninstallReviews.find(review =>
    review.receipt === reportReceipt && report?.app_key === p.appKey(review.app) &&
    (!reportApp || sameReviewApp(review.app, reportApp)));
  const rescanTarget = report && (reportReceipt !== undefined || shouldRescanAfterUninstall(report))
    ? reportReview && (canRescanReview(reportReview) || reportReview.rescan.phase === "scanning") ? reportReview.app : undefined
    : reportApp;
  useEffect(() => {
    if (liveReportApp && reportIdentity) {
      cacheReportApp(reportIdentity, liveReportApp);
    }
  }, [liveReportApp, reportIdentity]);
  const visibleReviews = p.uninstallReviews.filter(review =>
    review !== reportReview &&
    (review.restart || review.rescan.phase !== "complete" || review.rescan.count > 0 ||
      (p.selected ? sameReviewApp(review.app, p.selected) : review.receipt !== undefined && p.scanReviewReceipt === review.receipt)));
  return (
    <>
      <SoftwareToolbar
        searchRef={softwareSearch}
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
      <PendingUninstallReviews busy={p.busy || Boolean(p.uninstallingKey)} onGoOrphans={p.onGoOrphans} />
      {visibleReviews.length > 0 && (
        <section aria-label={L.uninstallReviewList} style={{ ...css.card, maxHeight: 224, overflowY: "auto", padding: 12, marginBottom: 10, lineHeight: 1.6 }}>
          {visibleReviews.map(review => {
            const waiting = review.restart === "before_verification";
            const live = p.apps.find(app => p.appKey(app) === p.appKey(review.app));
            const replaced = !!live && !sameReviewApp(live, review.app);
            const name = prettyAppName(review.app.name, review.app.source);
            const label = visibleReviews.some(other => other !== review && other.app.name === review.app.name)
              ? [name, review.app.version, review.app.install_location || review.app.source].filter(Boolean).join(" · ") : name;
            const alreadyVisible = review.receipt === p.scanReviewReceipt && review.rescan.phase === "complete";
            const retry = !replaced && canRescanReview(review) && !alreadyVisible &&
              (review.rescan.phase !== "complete" || review.rescan.count > 0);
            return (
              <div key={p.appKey(review.app)} style={{ marginBottom: 8 }}>
                <div role="status">
                  <strong>{waiting ? L.uninstallAwaitRestart : L.uninstallOk} · {label}</strong>
                  {review.restart && <div style={{ color: "var(--warn-ink)" }}>
                    {waiting ? L.backendUninstallRebootBeforeVerify : L.backendRebootRequired}
                  </div>}
                  {!waiting && <div style={{ color: "var(--muted)" }}>
                    {replaced ? L.uninstallReviewInstalled : uninstallRescanText(L, review.rescan, false)}
                  </div>}
                </div>
                {retry && <button type="button" style={{ ...css.btnGhost, marginTop: 4 }}
                  disabled={p.busy || !!p.uninstallingKey}
                  aria-label={`${L.reportRescanApp} · ${label}`}
                  onClick={() => p.listAnalyze(review.app, { fromUninstall: true, uninstallReview: review.receipt })}>
                  {L.reportRescanApp}
                </button>}
              </div>
            );
          })}
        </section>
      )}

      {p.selected && !p.scan && !p.detailPanel && (
        <SelectedAppCard
          selected={p.selected}
          showDetail={p.showDetail}
          onToggleDetail={p.onToggleDetail}
        />
      )}

      {p.error && <ErrorBanner error={p.error} onDismiss={() => p.setError(null)} />}

      {p.scan?.degraded && p.scan.degraded.length > 0 && (
        <div
          role="status"
          style={{ ...css.card, padding: "8px 12px", marginBottom: 10, color: "var(--warn-ink)", fontSize: 12 }}
        >
          {p.scan.degraded.map((code) => (
            <div key={code}>{degradedText(code)}</div>
          ))}
        </div>
      )}

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
          canRunOfficial={(p.scanTarget ?? p.selected)?.source !== "Monitor" && (p.scanTarget ?? p.selected)?.source !== "Orphan"}
          useOfficial={p.useOfficial}
          aiEnabled={p.aiEnabled}
          aiBusy={p.aiBusy}
          selectedPaths={p.selectedPaths}
          busy={p.busy}
          onBack={p.onBack}
          onUseOfficial={p.setUseOfficial}
          onDryRun={p.onDryRun}
          onCleanup={p.onCleanup}
          onCancelCleanup={p.onCancelCleanup}
          onAiExplain={p.onAiExplain}
          onOpenSettings={p.onOpenSettings}
          aiNote={p.aiSummaryNote}
        />
      )}

      {p.scan && p.previewStale && (
        <p role="status" className="scan-plan-stale">{L.cleanupPreviewStale}</p>
      )}

      {p.aiRisk && p.scan && (
        <AiRiskBanner risk={p.aiRisk} onDismiss={() => p.setAiRisk(null)} />
      )}

      {report && (
        <ReportPanel
          report={report}
          aiEnabled={p.aiEnabled}
          aiReportBusy={p.aiReportBusy}
          aiReportNote={p.aiReportNote}
          verifyRows={p.verifyRows}
          residualScan={reportReview?.rescan}
          residualRestart={reportReview?.restart}
          onRescan={
            rescanTarget
              ? () => p.listAnalyze(rescanTarget, {
                  afterCleanup: !!reportReview,
                  ...(reportReview && { uninstallReview: reportReview.receipt }),
                  keepReport: true,
                  reviewSession: reviewSessionFor(report),
                })
              : undefined
          }
          rescanBusy={p.busy || !!p.uninstallingKey}
          deepCheckApp={reportApp ? { name: reportApp.name } : report ? { name: report.app_name } : null}
          appInstallLocation={reportApp?.install_location}
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
          orphanCompleteness={p.checkupOrphanCompleteness}
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
                new Set(p.scan!.items.filter(defaultSelectable).map(cleanupItemKey)),
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
        <div className="software-inventory" ref={inventoryRef} data-detail-open={hasInventoryDetail || undefined}>
          <div
            className="software-inventory-list"
            style={{
              ...css.card,
              flex: 1,
              minWidth: 0,
              minHeight: 0,
              display: "flex",
              flexDirection: "column",
            }}
          >
            {p.inventoryIncomplete && (
              <div style={{ display: "flex", gap: 12, alignItems: "center", justifyContent: "space-between", padding: "10px 12px", color: "var(--warn-ink)" }}>
                <span role="status">{p.apps.length ? L.appInventoryPartial : L.appInventoryUnreadable}</span>
                {p.refreshInventory && (
                  <button
                    type="button"
                    style={css.btnSm}
                    disabled={inventoryRefreshing}
                    onClick={() => {
                      setInventoryRefreshing(true);
                      void p.refreshInventory?.().finally(() => setInventoryRefreshing(false));
                    }}
                  >
                    {inventoryRefreshing ? L.loadingGeneric : L.retry}
                  </button>
                )}
              </div>
            )}
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
            {(!p.inventoryIncomplete || p.apps.length > 0) && <SoftwareListTable
              filtered={p.filtered}
              allApps={p.apps}
              loading={p.loading}
              q={p.q}
              category={p.category}
              onResetSearch={() => {
                p.onQuery("");
                p.onCategory("all");
                softwareSearch.current?.focus();
              }}
              sortCol={p.sortCol}
              sortDesc={p.sortDesc}
              selected={p.selected}
              multi={p.multi}
              uninstallingKey={p.uninstallingKey}
              appKey={p.appKey}
              sizeText={p.formatAppSize}
              sizeOf={p.sizeOf}
              sortBy={p.sortBy}
              selectApp={app => {
                if (document.activeElement instanceof HTMLElement && inventoryRef.current?.contains(document.activeElement)) {
                  lastListFocus.current = document.activeElement;
                }
                p.selectApp(app);
              }}
              startUninstall={p.listStartUninstall}
              analyze={p.listAnalyze}
              forceClean={p.listForceClean}
              doIgnoreApp={p.listIgnoreApp}
              doIgnorePublisher={p.listIgnorePublisher}
              toggleMulti={p.toggleMulti}
              setSelected={p.setSelected}
            />}
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
