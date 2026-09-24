import { t } from "../i18n";
import { HistoryPanel } from "./HistoryPanel";
import { RestorePanel } from "./RestorePanel";
import { MonitorPanel } from "./MonitorPanel";
import { AiSettingsPanel } from "./AiSettingsPanel";
import { WhitelistPanel } from "./WhitelistPanel";
import { IdleRadarPanel } from "./IdleRadarPanel";
import { ScopedScanPanel } from "./ScopedScanPanel";
import { DiskRadarPanel } from "./DiskRadarPanel";
import { api } from "../lib/api";
import type { CloseMode } from "../lib/closeMode";
import type { FullCleanupReport, InstalledApp } from "../types";
import { ToolCard, type ToolItem } from "./MoreToolCard";
import { Section } from "./MoreSection";
import { useMoreHistory } from "../hooks/useMoreHistory";
import { useMoreRestore } from "../hooks/useMoreRestore";
import { useMoreTools } from "../hooks/useMoreTools";

export function MorePage({
  selected,
  monitoring,
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
  onGoSoftware,
  onLastReport,
}: {
  selected: InstalledApp | null;
  monitoring: boolean;
  monitorDiff: { added_files: string[]; added_reg_values: string[] } | null;
  lastReport: FullCleanupReport | null;
  closeMode: CloseMode | null;
  onCloseModeChange: (m: CloseMode) => void;
  onIgnorePublisher: () => void;
  onToggleMonitor: () => void;
  onMonitorToCleanup: () => void;
  onDismissMonitor: () => void;
  onExportReport: () => void;
  onError: (msg: string) => void;
  onCheckUpdate: () => void;
  onGoSoftware: () => void;
  onLastReport?: (r: FullCleanupReport) => void;
}) {
  const L = t();
  const tools = useMoreTools({ selected, onGoSoftware });
  const hist = useMoreHistory(onError);
  const rest = useMoreRestore(onError);
  const openTool = tools.openTool;

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
            badge: L.badgeNew,
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

  const advanced: ToolItem[] = [
    {
      id: "idle",
      title: L.idleTitle,
      desc: L.idleHint,
      icon: "idle",
      action: () => tools.setOpenTool("idle"),
    },
    {
      id: "installers",
      title: L.installerTitle,
      desc: L.installerHint,
      icon: "installers",
      action: () => tools.setOpenTool("installers"),
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
      action: () => tools.setOpenTool("toolcache"),
    },
    {
      id: "monitor",
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
  ];

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
      {openTool === "history" && (
        <HistoryPanel
          history={hist.history}
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
        />
      )}
      {openTool === "ai" && <AiSettingsPanel onClose={() => tools.setOpenTool(null)} />}
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
      {openTool === "diskradar" && (
        <DiskRadarPanel onClose={() => tools.setOpenTool(null)} onError={onError} />
      )}
      {/* REV-UX-02: always render the monitor tool surface (not only after a diff arrives). */}
      {(openTool === "monitor" || monitorDiff) && (
        <MonitorPanel
          diff={monitorDiff ?? { added_files: [], added_reg_values: [] }}
          monitoring={monitoring}
          onToCleanup={onMonitorToCleanup}
          onDismiss={() => {
            onDismissMonitor();
            if (openTool === "monitor") tools.setOpenTool(null);
          }}
          onClose={() => tools.setOpenTool(null)}
        />
      )}
      <Section title={L.moreSectionCommon} hint={L.moreSectionCommonHint}>
        {common.map((c) => (
          <ToolCard key={c.id} item={c} active={openTool === c.id} selectedApp={selected} />
        ))}
      </Section>

      <Section title={L.moreSectionAdvanced} hint={L.moreSectionAdvancedHint}>
        {advanced.map((c) => (
          <ToolCard key={c.id} item={c} active={openTool === c.id} selectedApp={selected} />
        ))}
      </Section>

      <Section title={L.moreSectionHelp} hint={L.moreSectionHelpHint}>
        {help.map((c) => (
          <ToolCard key={c.id} item={c} active={openTool === c.id} selectedApp={selected} />
        ))}
      </Section>

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
