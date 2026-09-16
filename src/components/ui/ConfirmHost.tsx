import { useEffect, useState, useSyncExternalStore, type CSSProperties } from "react";
import { t } from "../../i18n";
import {
  getConfirm,
  settleConfirm,
  subscribeConfirm,
  type ConfirmOptions,
} from "../../lib/confirm";

function HoldButton({
  label,
  holdMs,
  danger,
  disabled,
  onDone,
}: {
  label: string;
  holdMs: number;
  danger?: boolean;
  disabled?: boolean;
  onDone: () => void;
}) {
  const [progress, setProgress] = useState(0);
  const [holding, setHolding] = useState(false);

  useEffect(() => {
    if (!holding) return;
    const start = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const p = Math.min(1, (now - start) / holdMs);
      setProgress(p);
      if (p >= 1) {
        setHolding(false);
        setProgress(0);
        onDone();
        return;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [holding, holdMs, onDone]);

  const base: CSSProperties = {
    height: 36,
    padding: "0 16px",
    borderRadius: 8,
    border: "1px solid transparent",
    fontWeight: 600,
    fontSize: 13,
    cursor: disabled ? "not-allowed" : "pointer",
    position: "relative",
    overflow: "hidden",
    opacity: disabled ? 0.45 : 1,
    background: danger ? "var(--danger)" : "var(--accent)",
    color: danger ? "#1a0505" : "var(--accent-ink)",
    minWidth: 108,
    whiteSpace: "nowrap" as const,
  };

  return (
    <button
      type="button"
      style={base}
      disabled={disabled}
      onMouseDown={() => {
        if (holdMs > 0 && !disabled) setHolding(true);
      }}
      onMouseUp={() => setHolding(false)}
      onMouseLeave={() => setHolding(false)}
      onClick={() => {
        if (holdMs <= 0 && !disabled) onDone();
      }}
    >
      {progress > 0 && (
        <span
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            bottom: 0,
            width: `${progress * 100}%`,
            background: "rgba(0,0,0,.18)",
            pointerEvents: "none",
          }}
        />
      )}
      <span style={{ position: "relative" }}>
        {holdMs > 0 && holding
          ? t().confirmHoldHint
          : holdMs > 0
            ? t().confirmHoldStart
            : label}
      </span>
    </button>
  );
}

function ConfirmBody({ opts }: { opts: ConfirmOptions }) {
  const L = t();
  const holdMs = opts.danger && (opts.holdMs ?? 600) > 0 ? (opts.holdMs ?? 600) : 0;
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={opts.title}
      style={{
        width: "min(440px, calc(100vw - 32px))",
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
      <div style={{ fontSize: 15, fontWeight: 700, letterSpacing: -0.2 }}>{opts.title}</div>
      {opts.message && (
        <div
          style={{
            fontSize: 13,
            lineHeight: 1.55,
            color: "var(--muted)",
            whiteSpace: "pre-wrap" as const,
          }}
        >
          {opts.message}
        </div>
      )}
      <div
        style={{
          display: "flex",
          justifyContent: "flex-end",
          gap: 8,
          marginTop: 4,
        }}
      >
        <button
          type="button"
          style={{
            height: 36,
            padding: "0 14px",
            borderRadius: 8,
            border: "1px solid var(--border)",
            background: "transparent",
            color: "var(--fg)",
            fontSize: 13,
            cursor: "pointer",
          }}
          onClick={() => settleConfirm(false)}
        >
          {opts.cancelLabel || L.cancel}
        </button>
        <HoldButton
          label={opts.confirmLabel || L.confirmOk}
          holdMs={holdMs}
          danger={opts.danger}
          onDone={() => settleConfirm(true)}
        />
      </div>
    </div>
  );
}

export function ConfirmHost() {
  const opts = useSyncExternalStore(subscribeConfirm, getConfirm, getConfirm);
  useEffect(() => {
    if (!opts) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        settleConfirm(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [opts]);

  if (!opts) return null;
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
        if (e.target === e.currentTarget) settleConfirm(false);
      }}
    >
      <ConfirmBody opts={opts} />
    </div>
  );
}
