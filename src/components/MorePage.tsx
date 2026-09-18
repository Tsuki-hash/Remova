import { useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError, prettyAppName } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { HistoryPanel } from "./HistoryPanel";
import { RestorePanel } from "./RestorePanel";
import { MonitorPanel } from "./MonitorPanel";
import { AiSettingsPanel } from "./AiSettingsPanel";
import type { CloseMode } from "../lib/closeMode";
import {
  loadRescanAfterUninstall,
  saveRescanAfterUninstall,
} from "../lib/rescanPref";
import type { FullCleanupReport, InstalledApp } from "../types";
import { ToolCard, type ToolId, type ToolItem } from "./MoreToolCard";

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <section style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <header
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 10,
          paddingBottom: 6,
          borderBottom: "1px solid var(--border)",
        }}
      >
        <span style={{ fontWeight: 700, fontSize: 13.5, letterSpacing: 0.1 }}>{title}</span>
        {hint && <span style={{ ...css.muted, fontSize: 12 }}>{hint}</span>}
      </header>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))",
          gap: 10,
        }}
      >
        {children}
      </div>
    </section>
  );
}

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
  const [openTool, setOpenTool] = useState<ToolId | null>(null);
  const [rescanOn, setRescanOn] = useState(() => loadRescanAfterUninstall());
  const [history, setHistory] = useState<
    {
      app_name: string;
      deleted: number;
      failed: number;
      skipped?: number;
      backup_dir: string;
      created_at?: string;
    }[]
  >([]);
  const [histQ, setHistQ] = useState("");
  const [restoreSessions, setRestoreSessions] = useState<
    { name: string; size_kb: number; created_at: string }[]
  >([]);
  const [restorePick, setRestorePick] = useState("");
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreMsgs, setRestoreMsgs] = useState<string[]>([]);

  const openHistory = async () => {
    try {
      const h = await api.history();
      setHistory(h);
      setOpenTool("history");
    } catch (e) {
      onError(formatError(e));
    }
  };

  const openRestore = async () => {
    setRestoreMsgs([]);
    setRestorePick("");
    setRestoreSessions([]);
    try {
      const sessions = await api.backupSessions();
      setRestoreSessions(sessions);
      if (sessions[0]) setRestorePick(sessions[0].name);
      setOpenTool("restore");
    } catch (e) {
      onError(formatError(e));
    }
  };

  const exportCsv = async () => {
    try {
      const csv = await api.exportHistoryCsv();
      const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "remova-history.csv";
      a.click();
      URL.revokeObjectURL(url);
      toast.success(L.exportCsv);
    } catch (e) {
      onError(formatError(e));
    }
  };

  const deleteSession = async (name: string) => {
    try {
      await api.deleteBackupSession(name);
      const sessions = await api.backupSessions();
      setRestoreSessions(sessions);
      if (restorePick === name) setRestorePick(sessions[0]?.name ?? "");
    } catch (e) {
      onError(formatError(e));
    }
  };

  const runRestore = async () => {
    if (!restorePick || restoreBusy) return;
    const ok = await requestConfirm({
      title: L.restore,
      message: L.restoreConfirm,
      confirmLabel: L.restoreRun,
      danger: true,
    });
    if (!ok) return;
    setRestoreBusy(true);
    setRestoreMsgs([]);
    try {
      const msgs = await api.restoreSessionByName(restorePick);
      setRestoreMsgs(msgs.length ? msgs : ["ok"]);
      toast.success(L.restoreResult);
    } catch (e) {
      setRestoreMsgs([formatError(e)]);
      toast.error(L.errInvokeFailed(formatError(e)));
    } finally {
      setRestoreBusy(false);
    }
  };

  const requireSelection = (run: () => void) => {
    if (!selected) {
      toast.info(L.selectRowHint);
      onGoSoftware();
      return;
    }
    run();
  };

  const common: ToolItem[] = [
    {
      id: "history",
      title: L.history,
      desc: L.historyHint,
      icon: "⏱",
      action: () => void openHistory(),
      accent: true,
    },
    {
      id: "restore",
      title: L.restore,
      desc: L.restoreHint,
      icon: "↩",
      action: () => void openRestore(),
      accent: true,
    },
    {
      id: "csv",
      title: L.exportCsv,
      desc: L.exportCsvHint,
      icon: "↓",
      action: () => void exportCsv(),
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
      action: () => requireSelection(onIgnorePublisher),
      needsSelection: true,
    },
  ];

  const advanced: ToolItem[] = [
    {
      id: "force",
      title: L.forceClean,
      desc: selected ? L.forceCleanHint : L.forceCleanHint,
      icon: "⌘",
      action: () => requireSelection(onForceClean),
      needsSelection: true,
    },
    {
      id: "orphan",
      title: L.orphanScan,
      desc: L.orphanScanHint,
      icon: "⌕",
      action: onOrphanScan,
      accent: true,
    },
    {
      id: "monitor",
      title: monitoring ? L.monitorStop : L.monitorInstall,
      desc: monitoring ? L.monitorStopHint : L.monitorInstallHint,
      icon: monitoring ? "■" : "●",
      action: onToggleMonitor,
      accent: monitoring,
      badge: monitoring ? L.badgeRunning : undefined,
    },
    {
      id: "shell",
      title: shellMenu ? L.shellUnregister : L.shellMenu,
      desc: shellMenu ? L.shellUnregisterHint : L.shellMenuHint,
      icon: "☰",
      action: onShellToggle,
    },
    {
      id: "ai",
      title: L.aiSettings,
      desc: L.aiSettingsHint,
      icon: "✦",
      action: () => setOpenTool(openTool === "ai" ? null : "ai"),
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
      action: () => window.open("https://github.com/Tsuki-hash/Remova/releases", "_blank"),
    },
  ];

  const selectedLabel = selected
    ? prettyAppName(selected.name, selected.source)
    : L.moreSelectedNone;

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        gap: 18,
        overflow: "auto",
        paddingRight: 2,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          flexWrap: "wrap",
        }}
      >
        <span
          style={{
            fontSize: 11,
            fontWeight: 650,
            letterSpacing: 0.3,
            color: "var(--muted)",
            textTransform: "uppercase" as const,
          }}
        >
          {L.selectedAppChip}
        </span>
        <span
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 8,
            height: 30,
            padding: "0 12px",
            borderRadius: 999,
            border: `1px solid ${selected ? "var(--accent)" : "var(--border)"}`,
            background: selected ? "var(--accent-soft)" : "var(--surface)",
            color: selected ? "var(--accent)" : "var(--muted)",
            fontSize: 12.5,
            fontWeight: 600,
            maxWidth: 360,
          }}
        >
          <span className="ell">{selectedLabel}</span>
          {!selected && (
            <button
              type="button"
              onClick={onGoSoftware}
              style={{
                border: "none",
                background: "transparent",
                color: "var(--accent)",
                cursor: "pointer",
                fontWeight: 650,
                fontSize: 12,
                padding: 0,
              }}
            >
              {L.goToSoftware}
            </button>
          )}
        </span>
      </div>

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
              checked={rescanOn}
              onChange={(e) => {
                setRescanOn(e.target.checked);
                saveRescanAfterUninstall(e.target.checked);
              }}
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
            {(
              [
                { id: "tray" as const, label: L.closeModeTray },
                { id: "quit" as const, label: L.closeModeQuit },
              ]
            ).map((opt) => {
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
          history={history}
          histQ={histQ}
          setHistQ={setHistQ}
          onClose={() => setOpenTool(null)}
          onOpenBackup={async (path) => {
            try {
              await api.openPath(path);
            } catch (e) {
              onError(formatError(e));
            }
          }}
        />
      )}
      {openTool === "ai" && <AiSettingsPanel onClose={() => setOpenTool(null)} />}
      {openTool === "restore" && (
        <RestorePanel
          sessions={restoreSessions}
          pick={restorePick}
          setPick={setRestorePick}
          busy={restoreBusy}
          msgs={restoreMsgs}
          onRun={() => void runRestore()}
          onDelete={(name) => void deleteSession(name)}
          onClose={() => setOpenTool(null)}
        />
      )}
      {monitorDiff && (
        <MonitorPanel
          diff={monitorDiff}
          onToCleanup={onMonitorToCleanup}
          onDismiss={onDismissMonitor}
        />
      )}
    </div>
  );
}
