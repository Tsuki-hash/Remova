import { t } from "../i18n";
import { useCallback, useEffect, useRef, useState } from "react";
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
import { api, type IgnoreLists } from "../lib/api";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { cssStyles as css } from "../styles";
import type { CloseMode } from "../lib/closeMode";
import type { FullCleanupReport, InstalledApp } from "../types";
import { ToolCard, type ToolId, type ToolItem } from "./MoreToolCard";
import { DiskOverviewPanel } from "./DiskOverviewPanel";
import { CliGlobalsPanel } from "./CliGlobalsPanel";
import { MigratePanel } from "./MigratePanel";
import { loadRecentTools, saveRecentTools } from "../lib/recentTools";
import { Section } from "./MoreSection";
import { useMoreHistory } from "../hooks/useMoreHistory";
import { useMoreRestore } from "../hooks/useMoreRestore";
import { useMoreTools } from "../hooks/useMoreTools";

export function MorePage({
  selected,
  monitoring,
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
  selected: InstalledApp | null;
  monitoring: boolean;
  monitorSnapshotBusy?: boolean;
  monitorDiff: { added_files: string[]; added_reg_values: string[] } | null;
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
  const tools = useMoreTools({ selected, onGoSoftware });
  const hist = useMoreHistory(onError);
  const rest = useMoreRestore(onError);
  const [rebootGate, setRebootGate] = useState<string>("clear");
  const [filter, setFilter] = useState("");
  const [recent, setRecent] = useState<string[]>(loadRecentTools);
  const recordUse = useCallback((id: ToolId) => {
    setRecent((prev) => {
      const next = [id, ...prev.filter((x) => x !== id)].slice(0, 3);
      saveRecentTools(next);
      return next;
    });
  }, []);
  useEffect(() => {
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
  }, []);
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
  const { loadRestore } = rest;
  const { setOpenTool } = tools;
  useEffect(() => {
    if (!requestedBackup || handledBackup.current === requestedBackup.id) return;
    handledBackup.current = requestedBackup.id;
    setOpenTool("restore");
    void loadRestore(requestedBackup.name).finally(() => clearBackupSessionRequest(requestedBackup.id));
  }, [requestedBackup, loadRestore, setOpenTool]);
  const openTool = tools.openTool;
  const panelRef = useRef<HTMLDivElement>(null);
  const hasMonitorDiff = Boolean(monitorDiff);
  // `badge: L.badgeNew` used to stay lit for as long as `lastReport` was
  // retained, so "new" stopped meaning anything. Show it once per session, on the
  // first report that arrives.
  const seenReport = useRef(false);
  const reportIsNew = !!lastReport && !seenReport.current;
  useEffect(() => {
    if (lastReport) seenReport.current = true;
  }, [lastReport]);
  useEffect(() => {
    if (!openTool && !hasMonitorDiff) return;
    panelRef.current?.scrollIntoView?.({ block: "start" });
    panelRef.current?.focus({ preventScroll: true });
  }, [openTool, hasMonitorDiff]);

  const common: ToolItem[] = [
    {
      id: "history",
      title: L.history,
      desc: L.historyHint,
      icon: "history",
      action: () => {
 // Open the panel first so the click always has visible feedback.
        tools.setOpenTool("history");
        void hist.loadHistory();
      },
    },
    {
      id: "restore",
      title: L.restore,
      desc: L.restoreHint,
      icon: "restore",
      action: () => {
        tools.setOpenTool("restore");
        void rest.loadRestore();
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
    {
      id: "ignore",
      title: L.appWhitelist,
      desc: L.appWhitelistHint,
      icon: "whitelist",
      action: () => tools.setOpenTool("ignore"),
    },
  ];

  const installHealth: ToolItem[] = [
    {
      id: "broken",
      title: L.brokenInstallsTitle,
      desc: L.brokenInstallsHint,
      icon: "report",
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
      icon: "monitor" as const,
      action: () => {
        void onToggleMonitor();
        tools.setOpenTool("monitor");
      },
      badge: monitoring ? L.badgeRunning : undefined,
    },
  ];

  const space: ToolItem[] = [
    {
      id: "diskoverview",
      title: L.diskOverviewTitle,
      desc: L.diskOverviewHint,
      icon: "disk",
      action: () => tools.setOpenTool("diskoverview"),
    },
    {
      id: "cliglobals",
      title: L.cliTitle,
      desc: L.cliHint,
      icon: "toolcache",
      action: () => tools.setOpenTool("cliglobals"),
    },
    {
      id: "migrate",
      title: L.migrateTitle,
      desc: L.migrateHint,
      icon: "disk",
      action: () => tools.setOpenTool("migrate"),
    },
    {
      id: "diskradar",
      title: L.diskRadarTitle,
      desc: L.diskRadarHint,
      icon: "disk",
      action: () => tools.setOpenTool("diskradar"),
    },
    {
      id: "toolcache",
      title: L.toolcacheTitle,
      desc: L.toolcacheHint,
      icon: "toolcache",
      action: () => tools.toggleTool("toolcache"),
    },
    {
      id: "idle",
      title: L.idleTitle,
      desc: L.idleHint,
      icon: "idle",
      action: () => tools.setOpenTool("idle"),
    },
  ];

  const traces: ToolItem[] = [
    {
      id: "firewall",
      title: L.firewallTitle,
      desc: L.firewallHint,
      icon: "services",
      action: () => tools.toggleTool("firewall"),
    },
  ];

  const help: ToolItem[] = [
    {
      id: "ai",
      title: L.aiSettings,
      desc: L.aiSettingsHint,
      icon: "ai",
      action: () => tools.toggleTool("ai"),
    },
    {
      id: "releases",
      title: L.versionCheck,
      desc: L.versionCheckHint,
      icon: "update",
      action: onCheckUpdate,
    },
    {
      id: "onboarding",
      title: L.onboardingReopen,
      desc: L.onboardingReopenHint,
      icon: "report",
      action: () => onReopenOnboarding?.(),
    },
  ];

  // Recent-tool tracking wraps every action; filtering hides non-matching
  // cards and the sections that become empty.
  const track = (items: ToolItem[]) =>
    items.map((it) => ({
      ...it,
      action: () => {
        recordUse(it.id);
        it.action();
      },
    }));
  const q = filter.trim().toLowerCase();
  const match = (it: ToolItem) =>
    !q || it.title.toLowerCase().includes(q) || it.desc.toLowerCase().includes(q);
  const trackedCommon = track(common).filter(match);
  const trackedInstallHealth = track(installHealth).filter(match);
  const trackedSpace = track(space).filter(match);
  const trackedTraces = track(traces).filter(match);
  const trackedHelp = track(help).filter(match);
  const totalCards =
    common.length + installHealth.length + space.length + traces.length + help.length;
  const allTracked = [
    ...track(common),
    ...track(installHealth),
    ...track(space),
    ...track(traces),
    ...track(help),
  ];
  const recentCards = (!q ? recent : [])
    .map((id) => allTracked.find((c) => c.id === id))
    .filter((c): c is ToolItem => Boolean(c));

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
 // Not a flex column: children default to shrink:1 and squash the top card.
        display: "block",
        overflow: "auto",
        paddingRight: 2,
      }}
    >
      <div
        ref={panelRef}
        id="remova-more-tool-panel"
        role="region"
        aria-label={L.toolboxTitle}
        tabIndex={-1}
        style={{ outline: "none" }}
      >
      {openTool === "history" && (
        <HistoryPanel
          history={hist.history}
          loading={hist.historyLoading}
          loadError={hist.historyLoadError}
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
          unsealedCount={rest.unsealedSessions}
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
          onRun={() => void rest.runRestore()}
          onDelete={(name) => void rest.deleteSession(name)}
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
        <DiskOverviewPanel onClose={() => tools.setOpenTool(null)} />
      )}
      {openTool === "cliglobals" && (
        <CliGlobalsPanel onClose={() => tools.setOpenTool(null)} />
      )}
      {openTool === "migrate" && <MigratePanel onClose={() => tools.setOpenTool(null)} />}
      {openTool === "diskradar" && (
        <DiskRadarPanel onClose={() => tools.setOpenTool(null)} onError={onError} />
      )}
      {/* always render the monitor tool surface (not only after a diff arrives). */}
      {(openTool === "monitor" || monitorDiff) && (
        <MonitorPanel
          diff={monitorDiff ?? { added_files: [], added_reg_values: [] }}
          monitoring={monitoring}
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
      {totalCards > 8 && (
        <input
          aria-label={L.moreFilterTools}
          placeholder={L.moreFilterTools}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          style={{ ...css.input, height: 32, marginBottom: 4, minWidth: 180 }}
        />
      )}
      {recentCards.length > 0 && (
        <Section title={L.moreSectionRecent}>
          {recentCards.map((c) => (
            <ToolCard key={`recent-${c.id}`} item={c} active={openTool === c.id} />
          ))}
        </Section>
      )}
      {trackedCommon.length > 0 && (
        <Section title={L.moreSectionCommon} hint={L.moreSectionCommonHint}>
          {trackedCommon.map((c) => (
            <ToolCard key={c.id} item={c} active={openTool === c.id} />
          ))}
        </Section>
      )}
      {(trackedInstallHealth.length > 0 || (!q && rebootGate === "corrupt")) && (
        <Section title={L.moreSectionInstallHealth} hint={L.moreSectionInstallHealthHint}>
          {trackedInstallHealth.map((c) => (
            <ToolCard key={c.id} item={c} active={openTool === c.id} />
          ))}
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
      {trackedSpace.length > 0 && (
        <Section title={L.moreSectionSpace} hint={L.moreSectionSpaceHint}>
          {trackedSpace.map((c) => (
            <ToolCard key={c.id} item={c} active={openTool === c.id} />
          ))}
        </Section>
      )}
      {trackedTraces.length > 0 && (
        <Section title={L.moreSectionTraces} hint={L.moreSectionTracesHint}>
          {trackedTraces.map((c) => (
            <ToolCard key={c.id} item={c} active={openTool === c.id} />
          ))}
        </Section>
      )}
      {trackedHelp.length > 0 && (
        <Section
          title={L.moreSectionHelp}
          hint={L.moreSectionHelpHint}
          extra={
            <button
              style={{ ...css.btnGhost, height: 26, fontSize: 12 }}
              title={L.openReleases}
              onClick={() => {
                void import("../lib/updateCheck")
                  .then(({ RELEASES_URL }) => api.openPath(RELEASES_URL))
                  .catch((e) => {
                    console.error("[update] open releases failed", e);
                    toast.error(formatError(e));
                  });
              }}
            >
              {L.openReleases}
            </button>
          }
        >
          {trackedHelp.map((c) => (
            <ToolCard key={c.id} item={c} active={openTool === c.id} />
          ))}
        </Section>
      )}

      <Section title={L.closeMode} hint={L.closeModeHint}>
        <div style={{ gridColumn: "1 / -1" }}>
          <label
            style={{
              display: "flex",
              gap: 10,
              alignItems: "center",
              marginBottom: 12,
              fontSize: 13,
              color: "var(--fg)",
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              checked={tools.rescanOn}
              onChange={() => tools.toggleRescan()}
            />
            <span style={{ fontWeight: 600 }}>{L.rescanAfterUninstall}</span>
            <span style={{ color: "var(--muted)", fontSize: 12 }}>
              {L.rescanAfterUninstallHint}
            </span>
          </label>
          <label
            style={{
              display: "flex",
              gap: 10,
              alignItems: "center",
              marginBottom: 12,
              fontSize: 13,
              color: "var(--fg)",
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              checked={tools.restorePointOn}
              onChange={() => tools.toggleRestorePoint()}
            />
            <span style={{ fontWeight: 600 }}>{L.restorePointBeforeCleanup}</span>
            <span style={{ color: "var(--muted)", fontSize: 12 }}>
              {L.restorePointBeforeCleanupHint}
            </span>
          </label>
          <div
            style={{
              display: "inline-flex",
              border: "1px solid var(--border)",
              borderRadius: 10,
              overflow: "hidden",
              background: "var(--surface)",
            }}
          >
            {([
              { id: "tray" as const, label: L.closeModeTray },
              { id: "quit" as const, label: L.closeModeQuit },
            ]).map((opt) => {
              const active = closeMode === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  aria-pressed={active}
                  onClick={() => onCloseModeChange(opt.id)}
                  style={{
                    border: "none",
                    background: active ? "var(--accent-soft)" : "transparent",
                    color: active ? "var(--accent)" : "var(--fg)",
                    padding: "10px 18px",
                    fontSize: 13,
                    fontWeight: active ? 650 : 500,
                    cursor: "pointer",
                    borderRight: opt.id === "tray" ? "1px solid var(--border)" : "none",
                  }}
                >
                  {opt.label}
                </button>
              );
            })}
          </div>
        </div>
      </Section>
    </div>
  );
}
