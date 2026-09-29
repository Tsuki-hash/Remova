import { t } from "../i18n";
import { formatSize } from "../i18n";
import { cssStyles as css } from "../styles";

export type SessionInfo = {
  name: string;
  size_kb: number;
  created_at: string;
};

export function RestorePanel({
  sessions,
  loading,
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
  pick: string;
  setPick: (v: string) => void;
  busy: boolean;
  msgs: string[];
  onRun: () => void;
  onDelete: (name: string) => void;
  onClose: () => void;
}) {
  const L = t();
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 8 }}>
        <strong>{L.safetyVaultTitle}</strong>
        <span style={css.muted}>{L.safetyVaultHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button
            style={{ ...css.btn, height: 32, opacity: busy || !pick ? 0.5 : 1 }}
            disabled={busy || !pick}
            onClick={onRun}
          >
            {L.restoreRun}
          </button>
          <button style={{ ...css.btnGhost, height: 32 }} onClick={onClose}>
            {L.restoreClose}
          </button>
        </div>
      </div>
      {sessions.length === 0 ? (
        <div style={css.muted}>{loading ? L.loadingGeneric : L.restoreNoSessions}</div>
      ) : (
        <>
          <div style={{ ...css.muted, marginBottom: 6 }}>{L.restoreSelect}</div>
          <div style={{ maxHeight: 220, overflow: "auto" }}>
            {sessions.map((s) => (
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
                    onChange={() => setPick(s.name)}
                  />
                  <span style={{ flex: 1, minWidth: 0 }} className="ell" title={s.name}>
                    {s.name}
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
            ))}
          </div>
        </>
      )}
      {msgs.length > 0 && (
        <div style={{ marginTop: 10 }}>
          <strong>{L.restoreResult}</strong>
          <pre
            style={{
              margin: "6px 0 0",
              whiteSpace: "pre-wrap",
              fontSize: 12,
              color: "var(--muted)",
            }}
          >
            {msgs.slice(0, 20).join("\n")}
          </pre>
        </div>
      )}
    </div>
  );
}
