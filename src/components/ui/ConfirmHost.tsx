import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type CSSProperties, type PointerEvent } from "react";
import { t } from "../../i18n";
import { useDialogFocus } from "../../lib/useDialogFocus";
import {
  getConfirm,
  getConfirmChecked,
  setConfirmChecked,
  settleConfirm,
  subscribeConfirm,
} from "../../lib/confirm";

type HoldInput = { pointerId: number } | { key: string };

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
  const [hold, setHold] = useState<HoldInput | null>(null);
  const activeHold = useRef<HoldInput | null>(null);
  /** Set when a keyboard hold was released before the bar filled. */
  const [keyAborted, setKeyAborted] = useState(false);

  function startHold(input: HoldInput) {
    if (activeHold.current) return;
    activeHold.current = input;
    setKeyAborted(false);
    setHold(input);
  }

  const cancelHold = useCallback(() => {
    activeHold.current = null;
    setHold(null);
    setProgress(0);
  }, []);

  function endPointerHold(event: PointerEvent<HTMLButtonElement>) {
    const input = activeHold.current;
    if (input && "pointerId" in input && input.pointerId === event.pointerId) cancelHold();
  }

  useEffect(() => {
    if (!hold) return;
    const start = performance.now();
    let raf = 0;
    let current = true;
    const onVis = () => {
      if (document.visibilityState !== "visible") cancelHold();
    };
    window.addEventListener("blur", cancelHold);
    document.addEventListener("visibilitychange", onVis);
    const tick = (now: number) => {
      if (!current || activeHold.current !== hold) return;
      const p = Math.min(1, (now - start) / holdMs);
      if (p >= 1) {
        cancelHold();
        onDone();
        return;
      }
      setProgress(p);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      current = false;
      window.removeEventListener("blur", cancelHold);
      document.removeEventListener("visibilitychange", onVis);
      cancelAnimationFrame(raf);
    };
  }, [hold, holdMs, onDone, cancelHold]);

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
    // --danger-text doubles as the AA-safe button background: light #B91C1C
    // with surface text ≈5.9:1, dark #F87171 with dark text ≈7.1:1.
    background: danger ? "var(--danger-text)" : "var(--accent)",
    color: danger ? "var(--surface)" : "var(--accent-ink)",
    minWidth: 108,
    whiteSpace: "nowrap" as const,
  };

  return (
    <>
      {/* a keyboard tap of Enter/Space used to be silently inert on a
          hold-to-confirm button. Announce the keyboard path up front and say so
          when an early release aborts the hold. */}
      {holdMs > 0 && (
        <span role="status" style={{ fontSize: 12, color: "var(--muted)", lineHeight: 1.5, flexBasis: "100%", textAlign: "right" }}>
          {keyAborted ? t().confirmHoldKeyboardAborted : t().confirmHoldKeyboard}
        </span>
      )}
      <button
      type="button"
      style={base}
      disabled={disabled}
      aria-keyshortcuts="Enter Space"
      onPointerDown={(e) => {
        if (holdMs > 0 && !disabled && e.isPrimary && e.button === 0) {
          startHold({ pointerId: e.pointerId });
        }
      }}
      onPointerUp={endPointerHold}
      onPointerMove={(e) => {
        if ((e.buttons & 1) === 0) endPointerHold(e);
      }}
      onPointerLeave={endPointerHold}
      // A cancelled pointer or a focus change means the user is no longer
      // deliberately holding this button; finishing the countdown anyway would
      // turn "press and hold" into "press once, walk away".
      onPointerCancel={endPointerHold}
      onLostPointerCapture={endPointerHold}
      onBlur={cancelHold}
      onKeyDown={(e) => {
        if (holdMs > 0 && !disabled && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          if (!e.repeat) startHold({ key: e.key });
        }
      }}
      onKeyUp={(e) => {
        const input = activeHold.current;
        if (!input || !("key" in input) || input.key !== e.key) return;
        // Releasing before the bar fills is a real, reportable outcome — never a
        // silent no-op the user has to guess at.
        setKeyAborted(true);
        cancelHold();
      }}
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
        {holdMs > 0 && hold
          ? t().confirmHoldHint
          : holdMs > 0
            ? t().confirmHoldStart
            : label}
      </span>
    </button>
    </>
  );
}

function ConfirmBody({ opts }: { opts: NonNullable<ReturnType<typeof getConfirm>> }) {
  const L = t();
  const dialogRef = useRef<HTMLDivElement>(null);
  useDialogFocus(true, dialogRef);
  const holdMs = opts.danger && (opts.holdMs ?? 600) > 0 ? (opts.holdMs ?? 600) : 0;
  const [checked, setChecked] = useState(getConfirmChecked());
  const confirm = useCallback(() => settleConfirm(true, opts.id), [opts.id]);
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={opts.title}
      tabIndex={-1}
      ref={dialogRef}
      style={{
        outline: "none",
        width: "min(440px, calc(100vw - 32px))",
        maxHeight: "calc(100dvh - 32px)",
        overflow: "auto",
        overflowWrap: "anywhere",
        background: "var(--surface-elevated)",
        border: "1px solid var(--border)",
        borderRadius: 14,
        boxShadow: "var(--shadow-float)",
        padding: "24px",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      <div style={{ fontSize: 16, fontWeight: 600, letterSpacing: -0.2 }}>{opts.title}</div>
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
      {opts.checkbox && (
        <label
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 8,
            fontSize: 13,
            color: "var(--fg)",
            cursor: "pointer",
            userSelect: "none" as const,
          }}
        >
          <input
            type="checkbox"
            checked={checked}
            onChange={(e) => {
              setChecked(e.target.checked);
              setConfirmChecked(e.target.checked, opts.id);
            }}
            style={{ marginTop: 2, accentColor: "var(--accent)" }}
          />
          <span style={{ lineHeight: 1.45 }}>{opts.checkbox.label}</span>
        </label>
      )}
      {opts.checkbox?.status && <div role="status" style={{ fontSize: 12, color: "var(--muted)", lineHeight: 1.5 }}>
        {opts.checkbox.status(checked)}
      </div>}
      <div
        style={{
          display: "flex",
          justifyContent: "flex-end",
          flexWrap: "wrap",
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
          onClick={() => settleConfirm(false, opts.id)}
        >
          {opts.cancelLabel || L.cancel}
        </button>
        <HoldButton
          label={opts.confirmLabel || L.confirmOk}
          holdMs={holdMs}
          danger={opts.danger}
          onDone={confirm}
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
        settleConfirm(false, opts.id);
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
        animation: "remova-fade-in var(--dur-float) var(--ease-standard)",
        inset: 0,
        zIndex: 1000,
        background: "rgba(15,23,42,.42)",
        display: "grid",
        placeItems: "center",
        padding: 16,
        backdropFilter: "blur(2px)",
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) settleConfirm(false, opts.id);
      }}
    >
      <ConfirmBody key={opts.id} opts={opts} />
    </div>
  );
}
