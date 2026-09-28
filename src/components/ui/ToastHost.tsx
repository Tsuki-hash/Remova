import { useSyncExternalStore } from "react";
import { CloseGlyph } from "./Glyph";
import { t } from "../../i18n";
import { dismissToast, getToasts, subscribeToasts, type ToastItem } from "../../lib/toast";

const KIND_COLOR: Record<ToastItem["kind"], string> = {
  success: "var(--ok)",
  error: "var(--danger)",
  info: "var(--accent)",
};

const KIND_BG: Record<ToastItem["kind"], string> = {
  success: "color-mix(in srgb, var(--ok) 12%, var(--surface))",
  error: "color-mix(in srgb, var(--danger) 12%, var(--surface))",
  info: "var(--surface)",
};

export function ToastHost() {
  const items = useSyncExternalStore(subscribeToasts, getToasts, getToasts);
  if (!items.length) return null;
  const L = t();
  return (
    <div
      aria-live="polite"
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
          role={item.kind === "error" ? "alert" : "status"}
          style={{
            pointerEvents: "auto",
            display: "flex",
            gap: 10,
            alignItems: "flex-start",
            padding: "10px 12px",
            borderRadius: 10,
            border: `1px solid ${KIND_COLOR[item.kind]}`,
            background: KIND_BG[item.kind],
            boxShadow: "var(--shadow)",
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
                className="ell"
                style={{ color: "var(--muted)", fontSize: 12, marginTop: 2 }}
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
