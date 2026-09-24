import { t } from "../i18n";
import { CloseGlyph } from "./ui/Glyph";
import { cssStyles as css } from "../styles";

export function ErrorBanner({ error, onDismiss }: { error: string; onDismiss: () => void }) {
  const L = t();
  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 12,
        marginBottom: 12,
        padding: "10px 12px",
        borderRadius: 10,
        border: "1px solid #fca5a5",
        background: "var(--surface)",
        color: "var(--danger)",
        fontSize: 13,
        lineHeight: 1.45,
        flexShrink: 0,
        flexBasis: "auto",
        minHeight: 40,
        overflow: "visible",
      }}
      role="alert"
    >
      <span style={{ flex: 1 }}>{error}</span>
      <button
        style={{
          ...css.btnGhost,
          height: 28,
          padding: "0 10px",
          color: "var(--danger)",
          flexShrink: 0,
        }}
        onClick={onDismiss}
      >
        {L.errorDismiss}
      </button>
    </div>
  );
}

export function AiRiskBanner({ risk, onDismiss }: { risk: string; onDismiss: () => void }) {
  const L = t();
  return (
    <div
      style={{
        marginBottom: 8,
        padding: "8px 12px",
        borderRadius: 8,
        background: "var(--accent-soft)",
        color: "var(--fg)",
        fontSize: 12.5,
        lineHeight: 1.5,
        flexShrink: 0,
      }}
    >
      <strong>{L.aiRiskTitle}: </strong>
      {risk}
      <span style={{ color: "var(--muted)", marginLeft: 8 }}>· {L.aiDisclaimer}</span>
      <button style={{ ...css.btnSm, marginLeft: 10, height: 24 }} onClick={onDismiss}>
        <CloseGlyph />
      </button>
    </div>
  );
}
