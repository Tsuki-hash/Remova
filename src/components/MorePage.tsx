import { t } from "../i18n";
import { HistoryPanel } from "./HistoryPanel";
import { RestorePanel } from "./RestorePanel";
import { MonitorPanel } from "./MonitorPanel";
import { AiSettingsPanel } from "./AiSettingsPanel";
import { WhitelistPanel } from "./WhitelistPanel";
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
      icon: "⏱",
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
      icon: "↩",
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
            icon: "⎙",
            action: onExportReport,
            badge: L.badgeNew,
          },
        ]
      : []),
    {
      id: "ignore",
      title: L.appWhitelist,
      desc: L.appWhitelistHint,
      icon: "∅",
      action: () => tools.setOpenTool("ignore"),
    },
  ];

  const advanced: ToolItem[] = [
    {
      id: "monitor",
      title: monitoring ? L.monitorStop : L.monitorInstall,
      desc: monitoring ? L.monitorStopHint : L.monitorInstallHint,
      icon: monitoring ? "■" : "●",
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
      icon: "✦",
      action: () => tools.toggleTool("ai"),
    },
    {
      id: "releases",
      title: L.versionCheck,
      desc: L.versionCheckHint,
      icon: "↑",
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
      {monitorDiff && (
        <MonitorPanel
          diff={monitorDiff}
          onToCleanup={onMonitorToCleanup}
          onDismiss={onDismissMonitor}
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
