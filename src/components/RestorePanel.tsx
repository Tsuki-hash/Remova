import { currentLang, t } from "../i18n";
import { formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import type { RestorePreview } from "../lib/api";

export type SessionInfo = {
  name: string;
  size_kb: number;
  created_at: string;
};

export function RestorePanel({
  sessions,
  loading,
  loadError,
  onReload,
  preview,
  previewLoading,
  previewError,
  pick,
  setPick,
  busy,
  msgs,
  onRun,
  onDelete,
  onClose,
}: {
  sessions: SessionInfo[];
  loading?: boolean;
  loadError?: string | null;
  onReload?: () => void;
  preview?: RestorePreview | null;
  previewLoading?: boolean;
  previewError?: string | null;
  pick: string;
  setPick: (v: string) => void;
  busy: boolean;
  msgs: string[];
  onRun: () => void;
  onDelete: (name: string) => void;
  onClose: () => void;
}) {
  const L = t();
  const selectedPreview = preview?.name === pick ? preview : null;
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12, marginBottom: 8 }}>
        <strong>{L.safetyVaultTitle}</strong>
        <span style={css.muted}>{L.safetyVaultHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button
            style={{ ...css.btn, height: 32, opacity: busy || !pick ? 0.5 : 1 }}
            disabled={busy || loading || !!loadError || !pick || previewLoading || !!previewError || !selectedPreview}
            onClick={onRun}
          >
            {L.restoreRun}
          </button>
          <button style={{ ...css.btnGhost, height: 32 }} disabled={busy} onClick={onClose}>
            {L.restoreClose}
          </button>
        </div>
      </div>
      {loadError ? (
        <div role="alert">
          <span>{L.restoreLoadFailed}: {loadError}</span>
          <button style={css.btnGhost} onClick={onReload}>{L.manageReload}</button>
        </div>
      ) : sessions.length === 0 ? (
        <div style={css.muted}>{loading ? L.loadingGeneric : L.restoreNoSessions}</div>
      ) : (
        <>
          <div style={{ ...css.muted, marginBottom: 6 }}>{L.restoreSelect}</div>
          <div style={{ maxHeight: 220, overflow: "auto" }}>
            {sessions.map((s) => {
              const date = /^\d{10}$/.test(s.created_at)
                ? new Date(Number(s.created_at) * 1000).toLocaleString(currentLang() === "zh" ? "zh-CN" : "en-US")
                : s.created_at || "—";
              return (
              <div
                key={s.name}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "6px 4px",
                  borderBottom: "1px solid var(--border)",
                }}
              >
                <label
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
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span className="ell" style={{ display: "block" }} title={s.name}>{s.name}</span>
                    <span style={{ ...css.muted, fontSize: 12 }}>{L.restoreCreated}: {date}</span>
                  </span>
                  <span style={{ ...css.muted, whiteSpace: "nowrap" }}>
                    {formatSize(s.size_kb)}
                  </span>
                </label>
                <button
                  style={{ ...css.btnGhost, height: 26, padding: "0 8px", color: "var(--danger)" }}
                  disabled={busy}
                  onClick={() => {
 // Confirm lives in useMoreRestore — no second dialog here.
                    onDelete(s.name);
                  }}
                >
                  {L.deleteSession}
                </button>
              </div>
            ); })}
          </div>
        </>
      )}
      {!!pick && !loadError && <section aria-label={L.restorePreviewTitle} style={{ marginTop: 12, paddingTop: 10, borderTop: "1px solid var(--border)" }}>
        <strong>{L.restorePreviewTitle}</strong>
        {previewLoading ? <p role="status" style={css.muted}>{L.loadingGeneric}</p>
          : previewError ? <div role="alert" style={{ marginTop: 8 }}>
            {L.restoreLoadFailed}: {previewError}
            <button style={css.btnGhost} disabled={busy} onClick={onReload}>{L.manageReload}</button>
          </div> : selectedPreview && <>
            <p style={{ margin: "8px 0" }}>{L.restorePreviewSummary(selectedPreview.files, selectedPreview.registry, selectedPreview.path_entries)}</p>
            {selectedPreview.existing > 0 && <p style={{ color: "var(--warn-ink)", margin: "6px 0" }}>{L.restoreConflict(selectedPreview.existing)}</p>}
            {selectedPreview.unavailable > 0 && <p style={{ color: "var(--warn-ink)", margin: "6px 0" }}>{L.restoreUnavailable(selectedPreview.unavailable)}</p>}
            <p style={{ ...css.muted, fontSize: 12, lineHeight: 1.5, margin: "6px 0" }}>{L.restorePreviewHint}</p>
            <details style={{ marginTop: 8 }}>
              <summary style={{ cursor: "pointer" }}>{L.restoreTargets}</summary>
              {selectedPreview.entries.length < selectedPreview.files && <p style={css.muted}>{L.restorePreviewLimited(selectedPreview.entries.length, selectedPreview.files)}</p>}
              <ul style={{ listStyle: "none", margin: "6px 0", padding: 0, maxHeight: 220, overflow: "auto" }}>
                {selectedPreview.entries.map((entry, index) => <li key={`${entry.target}-${index}`} style={{ display: "flex", flexWrap: "wrap", gap: 8, padding: "6px 0", borderBottom: "1px solid var(--border)" }}>
                  <span style={{ flex: "1 1 240px", overflowWrap: "anywhere", fontFamily: "var(--mono)", fontSize: 12 }}>{entry.target}</span>
                  <span style={{ color: entry.status === "available" ? "var(--muted)" : "var(--warn-ink)" }}>
                    {entry.status === "existing" ? L.restoreTargetExisting : entry.status === "missing" ? L.restoreTargetMissing : entry.status === "blocked" ? L.restoreTargetBlocked : L.restoreTargetAvailable}
                  </span>
                </li>)}
              </ul>
            </details>
          </>}
      </section>}
      {msgs.length > 0 && (
        <div style={{ marginTop: 10 }}>
          <strong>{L.restoreResult}</strong>
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
