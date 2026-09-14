import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export function RestorePanel({
  sessions,
  pick,
  setPick,
  busy,
  msgs,
  onRun,
  onClose,
}: {
  sessions: string[];
  pick: string;
  setPick: (v: string) => void;
  busy: boolean;
  msgs: string[];
  onRun: () => void;
  onClose: () => void;
}) {
  const L = t();
  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 8 }}>
        <strong>{L.restoreSessions}</strong>
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
        <div style={css.muted}>{L.restoreNoSessions}</div>
      ) : (
        <>
          <div style={{ ...css.muted, marginBottom: 6 }}>{L.restoreSelect}</div>
          <div style={{ maxHeight: 180, overflow: "auto" }}>
            {sessions.map((name) => (
              <label
                key={name}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "6px 4px",
                  borderBottom: "1px solid var(--border)",
                  cursor: "pointer",
                }}
              >
                <input
                  type="radio"
                  name="restore-session"
                  checked={pick === name}
                  onChange={() => setPick(name)}
                />
                <span>{name}</span>
              </label>
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
