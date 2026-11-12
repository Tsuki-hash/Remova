import { useLayoutEffect, useRef, useState } from "react";
import { currentLang, formatSize, t } from "../i18n";
import { cssStyles as css } from "../styles";
import type { RestorePreview } from "../lib/api";
import { backupDeleteExpectation, backupSessionStatus, canPreviewBackup, type BackupSession } from "../lib/backupSession";

type ActionFocus = { opener: HTMLButtonElement; nearby: string[] };

export function RestorePanel({
  sessions,
  loading,
  loadError,
  onReload,
  onRetryPreview,
  preview,
  previewLoading,
  previewError,
  pick,
  setPick,
  busy,
  msgs,
  resultName,
  runningRestore,
  cancelRequested,
  onCancelRestore,
  onRun,
  onDelete,
  onClose,
}: {
  sessions: BackupSession[];
  loading?: boolean;
  loadError?: string | null;
  onReload?: () => void;
  onRetryPreview?: () => void;
  preview?: RestorePreview | null;
  previewLoading?: boolean;
  previewError?: string | null;
  pick: string;
  setPick: (v: string) => void;
  busy: boolean;
  msgs: string[];
  resultName?: string;
  runningRestore?: string | null;
  cancelRequested?: boolean;
  onCancelRestore?: () => void;
  onRun: () => void | Promise<void>;
  onDelete: (name: string) => void | Promise<void>;
  onClose: () => void;
}) {
  const L = t();
  const panel = useRef<HTMLDivElement>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const [actionFocus, setActionFocus] = useState<ActionFocus | null>(null);
  useLayoutEffect(() => {
    if (busy || !actionFocus) return;
    setActionFocus(null);
    const { opener, nearby } = actionFocus;
    if (opener.ownerDocument.activeElement !== opener.ownerDocument.body) return;
    if (opener.isConnected && !opener.disabled) {
      opener.focus();
      return;
    }
    const rows = new Map(Array.from(panel.current?.querySelectorAll<HTMLElement>("[data-backup-session]") ?? [],
      (row) => [row.dataset.backupSession, row]));
    for (const name of nearby) {
      const control = rows.get(name)?.querySelector<HTMLElement>("input:enabled, button:enabled");
      if (control) {
        control.focus();
        return;
      }
    }
    title.current?.focus();
  }, [actionFocus, busy]);

  const actFrom = async (opener: HTMLButtonElement, action: () => void | Promise<void>, nearby: string[] = []) => {
    try {
      await action();
    } finally {
      setActionFocus({ opener, nearby });
    }
  };

  const selectable = sessions.some(canPreviewBackup);
  const selected = sessions.some(session => session.name === pick && canPreviewBackup(session));
  const selectedPreview = selected && preview?.name === pick ? preview : null;
  const canRestore = !busy && !loading && !loadError && !previewLoading && !previewError && Boolean(selectedPreview);
  const availability = {
    verified: "",
    pending_verification: L.restoreSessionPending,
    unsealed: L.restoreSessionUnsealed,
    requires_admin: L.restoreSessionRequiresAdmin,
    missing_metadata: L.restoreSessionMissingMetadata,
    unknown: L.restoreSessionUnknown,
    invalid: L.restoreSessionInvalid,
    legacy: L.restoreSessionLegacy,
  };
  const statusOrder = { missing: 0, blocked: 0, existing: 1, available: 2 };
  const previewEntries = [...(selectedPreview?.entries ?? [])]
    .sort((a, b) => statusOrder[a.status] - statusOrder[b.status]);
  return (
    <div ref={panel} style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12, marginBottom: 8 }}>
        <h2 ref={title} tabIndex={-1} style={{ margin: 0, fontSize: "inherit" }}>{L.safetyVaultTitle}</h2>
        <span style={css.muted}>{L.safetyVaultHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button
            style={{ ...css.btn, height: 32, opacity: canRestore ? 1 : 0.5 }}
            disabled={!canRestore}
            onClick={(event) => void actFrom(event.currentTarget, onRun)}
          >
            {L.restoreRun}
          </button>
          <button style={{ ...css.btnGhost, height: 32 }} disabled={busy} onClick={onClose}>
            {L.restoreClose}
          </button>
        </div>
      </div>
      {runningRestore && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
          <span role="status">{cancelRequested ? L.restoreCancelling : L.restoreRunning(runningRestore)}</span>
          <button style={css.btnGhost} disabled={cancelRequested} onClick={onCancelRestore}>{L.restoreCancel}</button>
        </div>
      )}
      {loadError && (
        <div role="alert">
          <span>{L.restoreLoadFailed}: {loadError}</span>
          <button style={css.btnGhost} disabled={busy} onClick={onReload}>{L.manageReload}</button>
        </div>
      )}
      {loading ? <div role="status" style={css.muted}>{L.loadingGeneric}</div>
        : sessions.length === 0 && !loadError ? <div style={css.muted}>{L.restoreNoSessions}</div> : null}
      {sessions.length > 0 && (
        <>
          {selectable && <div style={{ ...css.muted, marginBottom: 6 }}>{L.restoreSelect}</div>}
          <div style={{ maxHeight: 220, overflow: "auto" }}>
            {sessions.map((s, index) => {
              const status = backupSessionStatus(s);
              const verified = selectedPreview?.name === s.name;
              const size = s.size_kb ?? (verified ? selectedPreview.size_kb : null);
              const date = /^\d{10}$/.test(s.created_at)
                ? new Date(Number(s.created_at) * 1000).toLocaleString(currentLang() === "zh" ? "zh-CN" : "en-US")
                : s.created_at || "—";
              const details = <span style={{ flex: 1, minWidth: 0 }}>
                <span className="ell" style={{ display: "block" }} title={s.name}>{s.name}</span>
                <span style={{ ...css.muted, fontSize: 12 }}>{L.restoreCreated}: {date}{size != null && ` · ${formatSize(size)}`}</span>
                {availability[status] && !verified && <span style={{ display: "block", color: status === "pending_verification" ? "var(--muted)" : "var(--warn-ink)" }}>{availability[status]}</span>}
              </span>;
              return (
              <div
                key={s.name}
                data-backup-session={s.name}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "6px 4px",
                  borderBottom: "1px solid var(--border)",
                }}
              >
                {canPreviewBackup(s) ? <label
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    flex: 1,
                    minWidth: 0,
                    cursor: "pointer",
                  }}
                >
                  <input
                    type="radio"
                    name="restore-session"
                    checked={pick === s.name}
                    disabled={busy}
                    onChange={() => setPick(s.name)}
                  />
                  {details}
                </label> : details}
                {backupDeleteExpectation(s, verified) && <button
                  style={{ ...css.btnGhost, height: 26, padding: "0 8px", color: "var(--danger-text)" }}
                  disabled={busy}
                  aria-label={`${L.deleteSession}: ${s.name}`}
                  onClick={(event) => {
                    const nearby = [...sessions.slice(index + 1), ...sessions.slice(0, index).reverse()].map(({ name }) => name);
                    void actFrom(event.currentTarget, () => onDelete(s.name), nearby);
                  }}
                >
                  {L.deleteSession}
                </button>}
              </div>
            ); })}
          </div>
        </>
      )}
      {selected && !loadError && <section aria-label={L.restorePreviewTitle} style={{ marginTop: 12, paddingTop: 10, borderTop: "1px solid var(--border)" }}>
        <strong>{L.restorePreviewTitle}</strong>
        {previewLoading ? <p role="status" style={css.muted}>{L.loadingGeneric}</p>
          : previewError ? <div role="alert" style={{ marginTop: 8 }}>
            {L.restoreLoadFailed}: {previewError}
            <button style={css.btnGhost} disabled={busy} onClick={onRetryPreview ?? onReload}>{L.manageReload}</button>
          </div> : selectedPreview && <>
            <p style={{ margin: "8px 0" }}>{L.restorePreviewSummary(selectedPreview.files, selectedPreview.registry, selectedPreview.path_entries)}</p>
            {selectedPreview.existing > 0 && <p style={{ color: "var(--warn-ink)", margin: "6px 0" }}>{L.restoreConflict(selectedPreview.existing)}</p>}
            {selectedPreview.unavailable > 0 && <p style={{ color: "var(--warn-ink)", margin: "6px 0" }}>{L.restoreUnavailable(selectedPreview.unavailable)}</p>}
            <p style={{ ...css.muted, fontSize: 12, lineHeight: 1.5, margin: "6px 0" }}>{L.restorePreviewHint}</p>
            <details open={selectedPreview.existing > 0 || selectedPreview.unavailable > 0} style={{ marginTop: 8 }}>
              <summary style={{ cursor: "pointer" }}>{L.restoreTargets}</summary>
              {selectedPreview.entries.length < selectedPreview.files && <p style={css.muted}>{L.restorePreviewLimited(selectedPreview.entries.length, selectedPreview.files)}</p>}
              <ul style={{ listStyle: "none", margin: "6px 0", padding: 0, maxHeight: 220, overflow: "auto" }}>
                {previewEntries.map((entry, index) => <li key={`${entry.target}-${index}`} style={{ display: "flex", flexWrap: "wrap", gap: 8, padding: "6px 8px", borderBottom: "1px solid var(--border)",
                  borderLeft: entry.status === "available" ? undefined : "3px solid var(--warn-ink)", background: entry.status === "available" ? undefined : "var(--surface-2)" }}>
                  <span style={{ flex: "1 1 240px", overflowWrap: "anywhere", fontFamily: "var(--mono)", fontSize: 12 }}>{entry.target}</span>
                  <span style={{ color: entry.status === "available" ? "var(--muted)" : "var(--warn-ink)", fontWeight: entry.status === "available" ? 400 : 650 }}>
                    {entry.status === "existing" ? L.restoreTargetExisting : entry.status === "missing" ? L.restoreTargetMissing : entry.status === "blocked" ? L.restoreTargetBlocked : L.restoreTargetAvailable}
                  </span>
                </li>)}
              </ul>
            </details>
          </>}
      </section>}
      {msgs.length > 0 && (
        <div style={{ marginTop: 10 }}>
          <strong role="status">{resultName ? `${L.restoreResult}: ${resultName}` : L.restoreResult}</strong>
          <pre
            style={{
              margin: "6px 0 0",
              whiteSpace: "pre-wrap",
              fontSize: 12,
              color: "var(--muted)",
              maxHeight: 240,
              overflow: "auto",
            }}
          >
            {msgs.join("\n")}
          </pre>
        </div>
      )}
    </div>
  );
}
