import { useSyncExternalStore } from "react";
import { CloseGlyph } from "./Glyph";
import { t } from "../../i18n";
import { dismissToast, getToasts, subscribeToasts, type ToastItem } from "../../lib/toast";

const KIND_COLOR: Record<ToastItem["kind"], string> = {
  success: "var(--ok-ink)",
  error: "var(--danger-text)",
  info: "var(--accent-text)",
};

const KIND_BG: Record<ToastItem["kind"], string> = {
  success: "var(--surface)",
  error: "var(--surface)",
  info: "var(--surface)",
};

export function ToastHost() {
  const items = useSyncExternalStore(subscribeToasts, getToasts, getToasts);
  const L = t();
 // Keep the live region mounted so screen readers observe text changes.
  return (
    <div
      aria-live="polite"
      aria-atomic="false"
      style={{
        position: "fixed",
        right: 16,
        bottom: 16,
        zIndex: 900,
        display: "flex",
        flexDirection: "column",
        gap: 8,
        maxWidth: "min(420px, calc(100vw - 32px))",
        pointerEvents: "none",
      }}
    >
      {items.map((item) => (
        <div
          key={item.id}
          style={{
            pointerEvents: "auto",
            display: "flex",
            gap: 10,
            alignItems: "flex-start",
            padding: "12px 14px",
            borderRadius: 12,
            border: "1px solid var(--border)",
            borderLeft: `3px solid ${KIND_COLOR[item.kind]}`,
            background: KIND_BG[item.kind],
            boxShadow: "0 6px 24px rgba(0,0,0,.12)",
            fontSize: 13,
            lineHeight: 1.45,
          }}
        >
          <span
            style={{
              width: 8,
              height: 8,
              borderRadius: 99,
              background: KIND_COLOR[item.kind],
              marginTop: 6,
              flexShrink: 0,
            }}
          />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontWeight: 600, color: "var(--fg)" }}>{item.message}</div>
            {item.detail && (
              <div
                // The detail often carries the actionable reason — wrap it
                // for error/sticky toasts; keep one-line ellipsis for the rest.
                className={item.kind === "error" || item.sticky ? undefined : "ell"}
                style={{
                  color: "var(--muted)",
                  fontSize: 12,
                  marginTop: 2,
                  ...(item.kind === "error" || item.sticky
                    ? { whiteSpace: "normal" as const, wordBreak: "break-word" as const }
                    : {}),
                }}
                title={item.detail}
              >
                {item.detail}
              </div>
            )}
          </div>
          <button
            type="button"
            aria-label={L.errorDismiss}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--muted)",
              cursor: "pointer",
              fontSize: 16,
              lineHeight: 1,
              padding: 2,
              flexShrink: 0,
            }}
            onClick={() => dismissToast(item.id)}
          >
            <CloseGlyph />
          </button>
        </div>
      ))}
    </div>
  );
}
