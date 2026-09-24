import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { t } from "../../i18n";
import { useDialogFocus } from "../../lib/useDialogFocus";
import {
  isCloseChoiceOpen,
  saveCloseMode,
  settleCloseChoice,
  subscribeCloseChoice,
} from "../../lib/closeMode";

export function CloseChoiceHost() {
  const open = useSyncExternalStore(subscribeCloseChoice, isCloseChoiceOpen, isCloseChoiceOpen);
  const [remember, setRemember] = useState(true);
  const dialogRef = useRef<HTMLDivElement>(null);
  const L = t();
  useDialogFocus(open, dialogRef);

  useEffect(() => {
    if (!open) return;
    setRemember(true);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        settleCloseChoice("cancel");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open) return null;

  const pick = (mode: "tray" | "quit") => {
    if (remember) saveCloseMode(mode);
    settleCloseChoice(mode);
  };

  const btn = (primary?: boolean): React.CSSProperties => ({
    height: 36,
    padding: "0 14px",
    borderRadius: 8,
    border: primary ? "1px solid transparent" : "1px solid var(--border)",
    background: primary ? "var(--accent)" : "transparent",
    color: primary ? "var(--accent-ink)" : "var(--fg)",
    fontSize: 13,
    fontWeight: primary ? 650 : 500,
    cursor: "pointer",
    whiteSpace: "nowrap",
  });

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 1000,
        background: "rgba(15,23,42,.42)",
        display: "grid",
        placeItems: "center",
        backdropFilter: "blur(2px)",
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) settleCloseChoice("cancel");
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={L.closeChoiceTitle}
        tabIndex={-1}
        ref={dialogRef}
        style={{
          outline: "none",
          width: "min(420px, calc(100vw - 32px))",
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: 14,
          boxShadow: "var(--shadow)",
          padding: "18px 20px 16px",
          display: "flex",
          flexDirection: "column",
          gap: 12,
        }}
      >
        <div style={{ fontSize: 15, fontWeight: 700, letterSpacing: -0.2 }}>{L.closeChoiceTitle}</div>
        <div style={{ fontSize: 13, lineHeight: 1.55, color: "var(--muted)" }}>{L.closeChoiceBody}</div>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: 12.5,
            color: "var(--muted)",
            cursor: "pointer",
            userSelect: "none",
          }}
        >
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
            style={{ accentColor: "var(--accent)" }}
          />
          {L.closeChoiceRemember}
        </label>
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, flexWrap: "wrap" }}>
          <button type="button" style={btn()} onClick={() => settleCloseChoice("cancel")}>
            {L.cancel}
          </button>
          <button type="button" style={btn()} onClick={() => pick("quit")}>
            {L.closeModeQuit}
          </button>
          <button type="button" style={btn(true)} onClick={() => pick("tray")}>
            {L.closeModeTray}
          </button>
        </div>
      </div>
    </div>
  );
}
