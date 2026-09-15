import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { HistoryPanel } from "./HistoryPanel";
import { RestorePanel } from "./RestorePanel";
import { MonitorPanel } from "./MonitorPanel";
import type { FullCleanupReport, InstalledApp } from "../types";

type ToolId =
  | "history"
  | "restore"
  | "force"
  | "orphan"
  | "monitor"
  | "ignore"
  | "shell"
  | "releases"
  | "csv"
  | "report";

type ToolItem = {
  id: ToolId;
  title: string;
  desc: string;
  icon: string;
  action?: () => void;
  disabled?: boolean;
};

function ToolRow({
  item,
  active,
}: {
  item: ToolItem;
  active: boolean;
}) {
  return (
    <button
      type="button"
      disabled={item.disabled}
      onClick={item.action}
      style={{
        width: "100%",
        textAlign: "left" as const,
        border: "1px solid var(--border)",
        background: active ? "var(--accent-soft)" : "var(--surface)",
        borderColor: active ? "var(--accent)" : "var(--border)",
        borderRadius: 10,
        padding: "12px 14px",
        cursor: item.disabled ? "not-allowed" : "pointer",
        boxShadow: "var(--shadow)",
        opacity: item.disabled ? 0.55 : 1,
        display: "flex",
        gap: 12,
        alignItems: "flex-start",
      }}
    >
      <span
        style={{
          width: 32,
          height: 32,
          borderRadius: 8,
          background: "var(--surface-2)",
          display: "grid",
          placeItems: "center",
          fontSize: 15,
          flexShrink: 0,
        }}
        aria-hidden
      >
        {item.icon}
      </span>
      <span style={{ minWidth: 0, flex: 1 }}>
        <div style={{ fontWeight: 650, fontSize: 13.5, marginBottom: 3 }}>{item.title}</div>
        <div style={{ ...css.muted, fontSize: 12, lineHeight: 1.45 }}>{item.desc}</div>
      </span>
    </button>
  );
}

export function MorePage({
  selected,
  monitoring,
  monitorDiff,
  lastReport,
  shellMenu,
  onForceClean,
  onIgnorePublisher,
  onOrphanScan,
  onToggleMonitor,
  onMonitorToCleanup,
  onDismissMonitor,
  onShellToggle,
  onExportReport,
  onError,
}: {
  selected: InstalledApp | null;
  monitoring: boolean;
  monitorDiff: { added_files: string[]; added_reg_values: string[] } | null;
  lastReport: FullCleanupReport | null;
  shellMenu: boolean;
  onForceClean: () => void;
  onIgnorePublisher: () => void;
  onOrphanScan: () => void;
  onToggleMonitor: () => void;
  onMonitorToCleanup: () => void;
  onDismissMonitor: () => void;
  onShellToggle: () => void;
  onExportReport: () => void;
  onError: (msg: string) => void;
}) {
  const L = t();
  const [openTool, setOpenTool] = useState<ToolId | null>(null);
  const [history, setHistory] = useState<
    { app_name: string; deleted: number; failed: number; backup_dir: string }[]
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
      const h = await invoke<
        { app_name: string; deleted: number; failed: number; backup_dir: string }[]
      >("list_cleanup_history");
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
      const sessions = await invoke<{ name: string; size_kb: number; created_at: string }[]>(
        "list_backup_sessions",
      );
      setRestoreSessions(sessions);
      if (sessions[0]) setRestorePick(sessions[0].name);
      setOpenTool("restore");
    } catch (e) {
      onError(formatError(e));
    }
  };

  const exportCsv = async () => {
    try {
      const csv = await invoke<string>("export_history_csv");
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
      await invoke("delete_backup_session", { name });
      const sessions = await invoke<{ name: string; size_kb: number; created_at: string }[]>(
        "list_backup_sessions",
      );
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
      const msgs = await invoke<string[]>("restore_session_by_name", { name: restorePick });
      setRestoreMsgs(msgs.length ? msgs : ["ok"]);
      toast.success(L.restoreResult);
    } catch (e) {
      setRestoreMsgs([formatError(e)]);
      toast.error(L.errInvokeFailed(formatError(e)));
    } finally {
      setRestoreBusy(false);
    }
  };

  const common: ToolItem[] = [
    {
      id: "history",
      title: L.history,
      desc: L.historyHint,
      icon: "⏱",
      action: () => void openHistory(),
    },
    {
      id: "restore",
      title: L.restore,
      desc: L.restoreHint,
      icon: "↩",
      action: () => void openRestore(),
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
          },
        ]
      : []),
    {
      id: "ignore",
      title: L.ignorePub,
      desc: selected ? L.ignorePublisherHint : L.selectRowHint,
      icon: "∅",
      action: onIgnorePublisher,
      disabled: !selected?.publisher,
    },
  ];

  const advanced: ToolItem[] = [
    {
      id: "force",
      title: L.forceClean,
      desc: selected ? L.forceCleanHint : L.selectRowHint,
      icon: "⌘",
      action: onForceClean,
      disabled: !selected,
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
      action: onToggleMonitor,
    },
    {
      id: "shell",
      title: shellMenu ? L.shellUnregister : L.shellMenu,
      desc: shellMenu ? L.shellUnregisterHint : L.shellMenuHint,
      icon: "☰",
      action: onShellToggle,
    },
    {
      id: "releases",
      title: L.openReleases,
      desc: L.openReleasesHint,
      icon: "↗",
      action: () => window.open("https://github.com/Tsuki-hash/Remova/releases", "_blank"),
    },
  ];

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        gap: 14,
        overflow: "auto",
      }}
    >
      <section style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div>
          <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 2 }}>{L.moreSectionCommon}</div>
          <div style={css.muted}>{L.moreSectionCommonHint}</div>
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))", gap: 10 }}>
          {common.map((c) => (
            <ToolRow key={c.id} item={c} active={openTool === c.id} />
          ))}
        </div>
      </section>

      <section style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div>
          <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 2 }}>{L.moreSectionAdvanced}</div>
          <div style={css.muted}>{L.moreSectionAdvancedHint}</div>
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))", gap: 10 }}>
          {advanced.map((c) => (
            <ToolRow key={c.id} item={c} active={openTool === c.id} />
          ))}
        </div>
      </section>

      {openTool === "history" && (
        <HistoryPanel
          history={history}
          histQ={histQ}
          setHistQ={setHistQ}
          onClose={() => setOpenTool(null)}
        />
      )}
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
