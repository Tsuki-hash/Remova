import { t } from "../i18n";
import { useEffect, useRef, useState } from "react";
import { useBackupSessionRequest, clearBackupSessionRequest } from "../lib/backupSessionNavigation";
import { HistoryPanel } from "./HistoryPanel";
import { RestorePanel } from "./RestorePanel";
import { MonitorPanel } from "./MonitorPanel";
import { AiSettingsPanel } from "./AiSettingsPanel";
import { WhitelistPanel } from "./WhitelistPanel";
import { IdleRadarPanel } from "./IdleRadarPanel";
import { ScopedScanPanel } from "./ScopedScanPanel";
import { BrokenInstallsPanel } from "./BrokenInstallsPanel";
import { FirewallRulesPanel } from "./FirewallRulesPanel";
import { DiskRadarPanel } from "./DiskRadarPanel";
import { api, type IgnoreLists, type MonitorDiff } from "../lib/api";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { cssStyles as css } from "../styles";
import type { CloseMode } from "../lib/closeMode";
import type { FullCleanupReport, InstalledApp } from "../types";
import { ToolCard, type ToolItem } from "./MoreToolCard";
import { ToolGlyph } from "./ToolIcons";
import { DiskOverviewPanel } from "./DiskOverviewPanel";
import { CliGlobalsPanel } from "./CliGlobalsPanel";
import { MigratePanel } from "./MigratePanel";
import { TracesPanel } from "./TracesPanel";
import { MsiPanel, VectorsPanel } from "./AuditPanels";
import { ToolchainPanel } from "./ToolchainPanel";
import { Section } from "./MoreSection";
import { useMoreHistory } from "../hooks/useMoreHistory";
import type { useMoreRestore } from "../hooks/useMoreRestore";
import { useMoreTools } from "../hooks/useMoreTools";

export function MorePage({
  settingsOnly = false,
  restore: rest,
  selected,
  monitoring,
  monitorRecovery,
  onDiscardMonitorRecovery,
  monitorSnapshotBusy,
  monitorDiff,
  lastReport,
  closeMode,
  onCloseModeChange,
  onIgnorePublisher,
  onToggleMonitor,
  onMonitorToCleanup,
  onDismissMonitor,
  onExportReport,
  onError,
  onCheckUpdate,
  onReopenOnboarding,
  onGoSoftware,
  onLastReport,
  onAiEnabledChange,
  onIgnoreListsChange,
}: {
  settingsOnly?: boolean;
  restore: ReturnType<typeof useMoreRestore>;
  selected: InstalledApp | null;
  monitoring: boolean;
  monitorRecovery?: "recovered" | "unavailable" | null;
  onDiscardMonitorRecovery?: () => void;
  monitorSnapshotBusy?: boolean;
  monitorDiff: MonitorDiff | null;
  lastReport: FullCleanupReport | null;
  closeMode: CloseMode | null;
  onCloseModeChange: (m: CloseMode) => void;
  onIgnorePublisher: () => Promise<IgnoreLists | undefined>;
  onToggleMonitor: () => void;
  onMonitorToCleanup: () => void;
  onDismissMonitor: () => void;
  onExportReport: () => void;
  onError: (msg: string) => void;
  onCheckUpdate: () => void;
  onReopenOnboarding?: () => void;
  onGoSoftware: () => void;
  onLastReport?: (r: FullCleanupReport) => void;
  onAiEnabledChange?: (enabled: boolean) => void;
  onIgnoreListsChange?: (lists: IgnoreLists) => void;
}) {
  const L = t();
  const tools = useMoreTools({ selected, onGoSoftware, initialTool: !settingsOnly && rest.openRestore ? "restore" : null });
  const hist = useMoreHistory(onError);
  const [rebootGate, setRebootGate] = useState<string>("clear");
  const [filter, setFilter] = useState("");
  // A triage row can hand its path straight to the migration tool; opening
  // the card directly starts from a clean form instead.
  const [migrateSeed, setMigrateSeed] = useState("");
  useEffect(() => {
    if (settingsOnly) return;
    let alive = true;
    void api
      .rebootGateStatus()
      .then((state) => {
        if (alive) setRebootGate(state);
      })
      .catch(() => {
        // Older backends do not expose the status command.
      });
    return () => {
      alive = false;
    };
  }, [settingsOnly]);
  const resetRebootGate = async () => {
    try {
      await api.clearCorruptRebootGate();
      setRebootGate("clear");
      toast.success(L.rebootGateCorruptResetDone);
    } catch (e) {
      onError(formatError(e));
    }
  };
  const requestedBackup = useBackupSessionRequest();
  const handledBackup = useRef(0);
  const { loadRestore, restoreBusy, restoreLoading } = rest;
  const { openFromTrigger } = tools;
  const searchRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (settingsOnly || !requestedBackup || handledBackup.current === requestedBackup.id || restoreBusy || restoreLoading) return;
    handledBackup.current = requestedBackup.id;
    // A backup opened from another page has no card opener in this toolbox.
    openFromTrigger("restore", { opener: searchRef.current });
    void loadRestore(requestedBackup.name).finally(() => clearBackupSessionRequest(requestedBackup.id));
  }, [settingsOnly, requestedBackup, loadRestore, openFromTrigger, restoreBusy, restoreLoading]);
  const openTool = tools.openTool;
  const setPreviewVisible = rest.setPreviewVisible;
  useEffect(() => {
    if (openTool !== "restore") return;
    setPreviewVisible(true);
    return () => setPreviewVisible(false);
  }, [openTool, setPreviewVisible]);
  const panelRef = useRef<HTMLDivElement>(null);
  const catalogRef = useRef<HTMLDetailsElement>(null);
  const hasMonitorDiff = !settingsOnly && Boolean(monitorDiff);
  const showMonitorPanel = openTool === "monitor" || (hasMonitorDiff && !openTool);
  // `badge: L.badgeNew` used to stay lit for as long as `lastReport` was
  // retained, so "new" stopped meaning anything. Show it once per session, on the
  // first report that arrives.
  const seenReport = useRef(false);
  const reportIsNew = !!lastReport && !seenReport.current;
  useEffect(() => {
    if (lastReport) seenReport.current = true;
  }, [lastReport]);
  useEffect(() => {
    if (!openTool && !showMonitorPanel) return;
    panelRef.current?.scrollIntoView?.({ block: "start" });
    panelRef.current?.focus({ preventScroll: true });
  }, [openTool, showMonitorPanel]);
  useEffect(() => {
    if (catalogRef.current) catalogRef.current.open = !openTool;
  }, [openTool]);

  const quick: ToolItem[] = [
    {
      id: "restore",
      title: L.restore,
      desc: L.restoreHint,
      icon: "restore",
      action: () => {
        openFromTrigger("restore");
        void rest.loadRestore();
      },
    },
    {
      id: "history",
      title: L.history,
      desc: L.historyHint,
      icon: "history",
      action: () => {
 // Open the panel first so the click always has visible feedback.
        openFromTrigger("history");
        void hist.loadHistory();
      },
    },
    ...(lastReport
      ? [
          {
            id: "report" as const,
            title: L.exportReport,
            desc: L.exportReportHint,
            icon: "report" as const,
            action: onExportReport,
            badge: reportIsNew ? L.badgeNew : undefined,
          },
        ]
      : []),
  ];

  const installHealth: ToolItem[] = [
    {
      id: "broken",
      title: L.brokenInstallsTitle,
      desc: L.brokenInstallsHint,
      icon: "broken-link",
      action: () => tools.toggleTool("broken"),
    },
    {
      id: "installers",
      title: L.installerTitle,
      desc: L.installerHint,
      icon: "installers",
      // a second click on the highlighted card now closes the panel,
      // like the AI card — not a silent no-op.
      action: () => tools.toggleTool("installers"),
    },
    {
      id: "monitor",
      disabled: monitorSnapshotBusy,
      title: monitoring ? L.monitorStop : L.monitorInstall,
      desc: monitoring ? L.monitorStopHint : L.monitorInstallHint,
      icon: "target",
      action: () => {
        void onToggleMonitor();
        openFromTrigger("monitor");
      },
      badge: monitoring ? L.badgeRunning : monitorRecovery ? L.monitorRecoveryBadge : undefined,
    },
  ];

  const space: ToolItem[] = [
    {
      id: "diskoverview",
      title: L.diskOverviewTitle,
      desc: L.diskOverviewHint,
      icon: "drive",
      action: () => tools.toggleTool("diskoverview"),
    },
    {
      id: "diskradar",
      title: L.diskRadarTitle,
      desc: L.diskRadarHint,
      icon: "disk",
      action: () => tools.toggleTool("diskradar"),
    },
    {
      id: "toolcache",
      title: L.toolcacheTitle,
      desc: L.toolcacheHint,
      icon: "cleanup",
      action: () => tools.toggleTool("toolcache"),
    },
  ];

  const fileTools: ToolItem[] = [
    {
      id: "migrate",
      title: L.toolboxMigrate,
      desc: L.migrateHint,
      icon: "migrate",
      action: () => {
        setMigrateSeed("");
        tools.toggleTool("migrate");
      },
    },
    {
      id: "ignore",
      title: L.appWhitelist,
      desc: L.appWhitelistHint,
      icon: "whitelist",
      action: () => tools.toggleTool("ignore"),
    },
    {
      id: "cliglobals",
      title: L.toolboxCli,
      desc: L.cliHint,
      icon: "task-command",
      action: () => tools.toggleTool("cliglobals"),
    },
    {
      id: "idle",
      title: L.toolboxIdle,
      desc: L.idleHint,
      icon: "idle",
      action: () => tools.toggleTool("idle"),
    },
  ];

  const traces: ToolItem[] = [
    {
      id: "vectors",
      title: L.vectorsTitle,
      desc: L.vectorsHint,
      icon: "report",
      action: () => tools.toggleTool("vectors"),
    },
    {
      id: "msi",
      title: L.msiTitle,
      desc: L.msiHint,
      icon: "report",
      action: () => tools.toggleTool("msi"),
    },
    {
      id: "toolaudit",
      title: L.toolchainTitle,
      desc: L.toolchainHint,
      icon: "services",
      action: () => tools.toggleTool("toolaudit"),
    },
    {
      id: "firewall",
      title: L.firewallTitle,
      desc: L.firewallHint,
      icon: "services",
      action: () => tools.toggleTool("firewall"),
    },
    {
      id: "traces",
      title: L.tracesTitle,
      desc: L.tracesHint,
      icon: "report",
      action: () => tools.toggleTool("traces"),
    },
  ];

  const help: ToolItem[] = [
    {
      id: "releases",
      title: L.versionCheck,
      desc: L.versionCheckHint,
      icon: "update",
      action: onCheckUpdate,
    },
    {
      id: "open-releases",
      title: L.openReleases,
      desc: L.openReleasesHint,
      icon: "installers",
      action: () => {
        void import("../lib/updateCheck")
          .then(({ RELEASES_URL }) => api.openPath(RELEASES_URL))
          .catch((e) => {
            console.error("[update] open releases failed", e);
            toast.error(formatError(e));
          });
      },
    },
    {
      id: "onboarding",
      title: L.onboardingReopen,
      desc: L.onboardingReopenHint,
      icon: "report",
      action: () => onReopenOnboarding?.(),
    },
    {
      id: "ai",
      title: L.aiSettings,
      desc: L.aiSettingsHint,
      icon: "ai",
      action: () => tools.toggleTool("ai"),
    },
  ];

  const q = filter.trim().toLowerCase();
  const match = (it: ToolItem) =>
    !q || it.title.toLowerCase().includes(q) || it.desc.toLowerCase().includes(q);
  const trackedQuick = quick.filter(match);
  const trackedInstallHealth = installHealth.filter(match);
  const trackedSpace = space.filter(match);
  const trackedFiles = fileTools.filter(match);
  const trackedTraces = traces.filter(match);
  const trackedHelp = help.filter(match);
  const matchedCount = [trackedQuick, trackedInstallHealth, trackedSpace, trackedFiles, trackedTraces, trackedHelp]
    .reduce((count, group) => count + group.length, 0);
  const renderTool = (item: ToolItem) => (
    <ToolCard key={item.id} item={item} active={openTool === item.id} />
  );

  return (
    <div className="remova-toolbox">
      <div
        ref={panelRef}
        id="remova-more-tool-panel"
        role="region"
        aria-label={settingsOnly ? L.moreSectionHelp : L.toolboxTitle}
        tabIndex={-1}
        style={{ outline: "none" }}
      >
      {openTool === "history" && (
        <HistoryPanel
          history={hist.history}
          loading={hist.historyLoading}
          loadError={hist.historyLoadError}
          corruptRows={hist.historyCorruptRows}
          mutating={hist.historyMutating}
          onReload={() => void hist.loadHistory()}
          histQ={hist.histQ}
          setHistQ={hist.setHistQ}
          onDelete={(id) => void hist.deleteHistory(id)}
          onClearAll={() => void hist.clearHistory()}
          onClose={() => {
            hist.closeHistory();
            tools.setOpenTool(null);
          }}
        />
      )}
      {openTool === "restore" && (
        <RestorePanel
          sessions={rest.sessions}
          preview={rest.restorePreview}
          previewLoading={rest.previewLoading}
          previewError={rest.previewError}
          onRetryPreview={rest.retryPreview}
          loading={rest.restoreLoading}
          loadError={rest.restoreLoadError}
          onReload={() => void rest.loadRestore()}
          pick={rest.restorePick}
          setPick={rest.setRestorePick}
          busy={rest.restoreBusy}
          msgs={rest.restoreMsgs}
          resultName={rest.restoreResultName}
          runningRestore={rest.runningRestore}
          cancelRequested={rest.cancelRequested}
          onCancelRestore={() => void rest.cancelRestore()}
          onRun={rest.runRestore}
          onDelete={rest.deleteSession}
          onOpenFolder={(folder) => {
            void api.openPath(folder).catch((error) => onError(`${L.restoreOpenFolderFailed} ${formatError(error)}`));
          }}
          onClose={() => {
            rest.closeRestore();
            tools.setOpenTool(null);
          }}
        />
      )}
      {openTool === "ignore" && (
        <WhitelistPanel
          onClose={() => tools.setOpenTool(null)}
          onError={onError}
          onIgnorePublisher={onIgnorePublisher}
          onListsChange={onIgnoreListsChange}
        />
      )}
      {openTool === "ai" && <AiSettingsPanel onClose={() => tools.setOpenTool(null)} onEnabledChange={onAiEnabledChange} />}
      {openTool === "idle" && (
        <IdleRadarPanel
          onClose={() => tools.setOpenTool(null)}
          onGoSoftware={onGoSoftware}
          onError={onError}
        />
      )}
      {openTool === "installers" && (
        <ScopedScanPanel
          title={L.installerTitle}
          hint={L.installerHint}
          scan={api.scanInstallerCaches}
          cleanupSource="installer"
          appName={L.installerTitle}
          onClose={() => tools.setOpenTool(null)}
          onLastReport={(r) => onLastReport?.(r)}
          onError={onError}
        />
      )}
      {openTool === "toolcache" && (
        <ScopedScanPanel
          title={L.toolcacheTitle}
          hint={L.toolcacheHint}
          scan={api.scanToolCaches}
          cleanupSource="toolcache"
          appName={L.toolcacheTitle}
          onClose={() => tools.setOpenTool(null)}
          onLastReport={(r) => onLastReport?.(r)}
          onError={onError}
        />
      )}
      {openTool === "broken" && (
        <BrokenInstallsPanel onClose={() => tools.setOpenTool(null)} onError={onError} />
      )}
      {openTool === "firewall" && (
        <FirewallRulesPanel onClose={() => tools.setOpenTool(null)} onError={onError} />
      )}
      {openTool === "diskoverview" && (
        <DiskOverviewPanel
          onClose={() => tools.setOpenTool(null)}
          onMigrate={(path) => {
            setMigrateSeed(path);
            tools.setOpenTool("migrate");
          }}
        />
      )}
      {openTool === "cliglobals" && (
        <CliGlobalsPanel onClose={() => tools.setOpenTool(null)} />
      )}
      {openTool === "migrate" && (
        <MigratePanel onClose={() => tools.setOpenTool(null)} initialSource={migrateSeed} />
      )}
      {openTool === "traces" && <TracesPanel onClose={() => tools.setOpenTool(null)} />}
      {openTool === "vectors" && <VectorsPanel onClose={() => tools.setOpenTool(null)} />}
      {openTool === "msi" && <MsiPanel onClose={() => tools.setOpenTool(null)} />}
      {openTool === "toolaudit" && <ToolchainPanel onClose={() => tools.setOpenTool(null)} />}
      {openTool === "diskradar" && (
        <DiskRadarPanel onClose={() => tools.setOpenTool(null)} onError={onError} />
      )}
      {showMonitorPanel && (
        <MonitorPanel
          diff={monitorDiff}
          monitoring={monitoring}
          recovery={monitorRecovery}
          onDiscardRecovery={onDiscardMonitorRecovery}
          stopping={monitorSnapshotBusy}
          onToggle={onToggleMonitor}
          onToCleanup={onMonitorToCleanup}
          onDismiss={() => {
            onDismissMonitor();
            if (openTool === "monitor") tools.setOpenTool(null);
          }}
 // No separate Close: the panel also renders without openTool (once a
 // diff exists), where clearing openTool alone could not dismiss it.
        />
      )}
      </div>
      {hasMonitorDiff && !showMonitorPanel && (
        <div className="remova-tool-pending">
          <button type="button" style={css.btnGhost} onClick={() => openFromTrigger("monitor", { opener: searchRef.current })}>
            {L.monitorViewResult}
          </button>
        </div>
      )}
      {!settingsOnly && <details className="remova-tool-catalog" ref={catalogRef} open={!openTool}>
        <summary hidden={!openTool}>{L.moreBrowseTools}</summary>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            flexWrap: "wrap",
            marginBottom: 16,
          }}
        >
          <div
            style={{
              position: "relative",
              display: "flex",
              alignItems: "center",
              width: "min(100%, 380px)",
            }}
          >
            <span
              aria-hidden
              style={{
                position: "absolute",
                left: 11,
                display: "inline-flex",
                color: "var(--muted)",
                pointerEvents: "none",
              }}
            >
              <ToolGlyph name="monitor" />
            </span>
            <input
              ref={searchRef}
              type="text"
              aria-label={L.moreSearchTools}
              placeholder={L.moreSearchToolsHint}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") setFilter("");
              }}
              style={{
                ...css.input,
                width: "100%",
                minWidth: 0,
                height: 38,
                paddingLeft: 36,
                paddingRight: filter ? 34 : 12,
              }}
            />
            {filter && (
              <button
                type="button"
                aria-label={L.moreSearchClear}
                title={L.moreSearchClear}
                onClick={() => setFilter("")}
                style={{
                  position: "absolute",
                  right: 6,
                  width: 24,
                  height: 24,
                  border: "none",
                  borderRadius: 6,
                  background: "transparent",
                  color: "var(--muted)",
                  cursor: "pointer",
                  fontSize: 14,
                  lineHeight: 1,
                }}
              >
                ×
              </button>
            )}
          </div>
          {q && (
            <span role="status" style={{ ...css.muted, fontSize: 12 }}>
              {L.moreSearchResults(matchedCount)}
            </span>
          )}
        </div>
      {q && matchedCount === 0 && (
        <div style={{ ...css.muted, fontSize: 13, marginBottom: 16 }}>{L.moreSearchEmpty}</div>
      )}
      {trackedQuick.length > 0 && (
        <div className="remova-tool-quick">
          {trackedQuick.map((item) => (
            <ToolCard key={item.id} item={item} active={openTool === item.id} quick primary={item.id === "restore"} />
          ))}
        </div>
      )}
      <div className="remova-tool-groups">
      {trackedSpace.length > 0 && (
        <Section title={L.moreSectionSpace}>{trackedSpace.map(renderTool)}</Section>
      )}
      {(trackedInstallHealth.length > 0 || (!q && rebootGate === "corrupt")) && (
        <Section title={L.moreSectionInstallHealth}>
          {trackedInstallHealth.map(renderTool)}
          {rebootGate === "corrupt" && (
            <div
              style={{ ...css.card, borderColor: "var(--warn-ink)", marginTop: 10 }}
              role="region"
              aria-label={L.rebootGateCorruptTitle}
            >
              <strong>{L.rebootGateCorruptTitle}</strong>
              <p style={css.muted}>{L.rebootGateCorruptHint}</p>
              <button
                type="button"
                style={{ ...css.btnGhost, height: 30 }}
                onClick={() => void resetRebootGate()}
              >
                {L.rebootGateCorruptReset}
              </button>
            </div>
          )}
        </Section>
      )}
      </div>
      {trackedFiles.length > 0 && (
        <Section title={L.toolboxFileTools} columns>{trackedFiles.map(renderTool)}</Section>
      )}
      {trackedTraces.length > 0 && (
        <Section title={L.moreSectionTraces} columns>
          {trackedTraces.map(renderTool)}
        </Section>
      )}
      {q && trackedHelp.length > 0 && <Section title={L.moreSectionHelp} columns>
        {trackedHelp.map(renderTool)}
      </Section>}
      </details>}
      {settingsOnly && <div className="remova-settings-grid" hidden={Boolean(openTool)}>
        <div className="remova-settings-preferences">
          <div className="remova-settings-card">
            <Section title={L.cleanupPreferences}>
              {([
                ["rescan", L.rescanAfterUninstall, L.rescanAfterUninstallHint, tools.rescanOn, tools.toggleRescan],
                ["restore-point", L.restorePointBeforeCleanup, L.restorePointBeforeCleanupHint, tools.restorePointOn, tools.toggleRestorePoint],
              ] as const).map(([id, label, help, checked, onChange]) => (
                <label className="remova-setting-option" key={id}>
                  <span>
                    <strong>{label}</strong>
                    <span id={"remova-setting-help-" + id}>{help}</span>
                  </span>
                  <input
                    type="checkbox"
                    role="switch"
                    className="remova-setting-switch"
                    aria-label={label}
                    aria-describedby={"remova-setting-help-" + id}
                    checked={checked}
                    onChange={onChange}
                  />
                </label>
              ))}
            </Section>
          </div>
          <div className="remova-settings-card">
            <Section title={L.closeMode}>
              <div className="remova-setting-close">
                <div className="remova-segmented" role="group" aria-label={L.closeMode}>
                  {([["tray", L.closeModeTray], ["quit", L.closeModeQuit]] as const).map(([mode, label]) => (
                    <button key={mode} type="button" aria-pressed={closeMode === mode}
                      onClick={() => onCloseModeChange(mode)}>{label}</button>
                  ))}
                </div>
                {closeMode !== "quit" && <p className="remova-tool-note">
                  {closeMode === "tray" ? L.closeModeHint : L.closeModeAskHint}
                </p>}
              </div>
            </Section>
          </div>
        </div>
        <div className="remova-settings-card">
          <Section title={L.toolboxHelp}>
            {help.map(renderTool)}
          </Section>
        </div>
      </div>}
    </div>
  );
}
