import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { prettyAppName } from "../lib/format";
import { api } from "../lib/api";
import { toast } from "../lib/toast";
import { HistoryPanel } from "./HistoryPanel";
import { RestorePanel } from "./RestorePanel";
import { MonitorPanel } from "./MonitorPanel";
import { AiSettingsPanel } from "./AiSettingsPanel";
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
  shellMenu,
  closeMode,
  onCloseModeChange,
  onForceClean,
  onIgnorePublisher,
  onOrphanScan,
  onToggleMonitor,
  onMonitorToCleanup,
  onDismissMonitor,
  onShellToggle,
  onExportReport,
  onError,
  onCheckUpdate,
  onGoSoftware,
}: {
  selected: InstalledApp | null;
  monitoring: boolean;
  monitorDiff: { added_files: string[]; added_reg_values: string[] } | null;
  lastReport: FullCleanupReport | null;
  shellMenu: boolean;
  closeMode: CloseMode | null;
  onCloseModeChange: (m: CloseMode) => void;
  onForceClean: () => void;
  onIgnorePublisher: () => void;
  onOrphanScan: () => void;
  onToggleMonitor: () => void;
  onMonitorToCleanup: () => void;
  onDismissMonitor: () => void;
  onShellToggle: () => void;
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
      action: () => void hist.loadHistory().then(() => tools.setOpenTool("history")),
    },
    {
      id: "restore",
      title: L.restore,
      desc: L.restoreHint,
      icon: "↩",
      action: () => void rest.loadRestore().then(() => tools.setOpenTool("restore")),
    },
    {
      id: "csv",
      title: L.exportCsv,
      desc: L.exportCsvHint,
      icon: "↓",
      action: () => void hist.exportCsv(),
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
      title: L.ignorePub,
      desc: selected
        ? `${L.ignorePublisherHint} · ${prettyAppName(selected.name, selected.source)}`
        : L.ignorePublisherHint,
      icon: "∅",
      action: () => tools.requireSelection(onIgnorePublisher),
      needsSelection: true,
    },
  ];

  const advanced: ToolItem[] = [
    {
      id: "force",
      title: L.forceClean,
      desc: L.forceCleanHint,
      icon: "⌘",
      action: () => tools.requireSelection(onForceClean),
      needsSelection: true,
    },
    {
      id: "orphan",
      title: L.orphanScan,
      desc: L.orphanScanHint,
      icon: "⌕",
      action: onOrphanScan,
    },
    {
      id: "monitor",
      title: monitoring ? L.monitorStop : L.monitorInstall,
      desc: monitoring ? L.monitorStopHint : L.monitorInstallHint,
      icon: monitoring ? "■" : "●",
      action: () => {
        void onToggleMonitor();
        // FE-P0b: open monitor panel after stop when a diff exists (parent updates props async).
        tools.setOpenTool("monitor");
      },
      badge: monitoring ? L.badgeRunning : undefined,
    },
    {
      id: "shell",
      title: shellMenu ? L.shellUnregister : L.shellMenu,
      desc: shellMenu ? L.shellUnregisterHint : L.shellMenuHint,
      icon: "☰",
      action: onShellToggle,
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
    {
      id: "open-releases",
      title: L.openReleases,
      desc: L.openReleasesHint,
      icon: "↗",
      action: () => {
        // window.open is blocked in the Tauri webview — open via backend ShellExecute.
        void api
          .openPath("https://github.com/Tsuki-hash/Remova/releases")
          .then(() => toast.info(L.openReleasesToast))
          .catch(() => toast.error(L.errOpenPathFailed));
      },
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

      {openTool === "history" && (
        <HistoryPanel
          history={hist.history}
          histQ={hist.histQ}
          setHistQ={hist.setHistQ}
          onClose={hist.closeHistory}
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
          onClose={rest.closeRestore}
        />
      )}
      {monitorDiff && (
        <MonitorPanel
          diff={monitorDiff}
          onToCleanup={onMonitorToCleanup}
          onDismiss={onDismissMonitor}
        />
      )}
      {openTool === "ai" && <AiSettingsPanel onClose={() => tools.setOpenTool(null)} />}
    </div>
  );
}
