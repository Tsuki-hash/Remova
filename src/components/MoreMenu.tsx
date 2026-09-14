import { useEffect, useRef } from "react";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export type MoreMenuItem = {
  id: string;
  label: string;
  hint?: string;
  disabled?: boolean;
  danger?: boolean;
  onClick: () => void;
};

export function MoreMenu({
  open,
  onOpenChange,
  items,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  items: MoreMenuItem[];
}) {
  const L = t();
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onOpenChange(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onOpenChange(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onOpenChange]);

  return (
    <div ref={rootRef} style={{ position: "relative", flexShrink: 0 }}>
      <button
        style={css.btnGhost}
        aria-expanded={open}
        aria-haspopup="menu"
        title={L.moreMenuHint}
        onClick={() => onOpenChange(!open)}
      >
        {L.moreMenu} {open ? "▴" : "▾"}
      </button>
      {open && (
        <div
          role="menu"
          style={{
            position: "absolute",
            right: 0,
            top: "calc(100% + 4px)",
            zIndex: 20,
            minWidth: 220,
            maxHeight: "min(70vh, 480px)",
            overflowY: "auto",
            background: "var(--surface)",
            border: "1px solid var(--border)",
            borderRadius: 10,
            boxShadow: "var(--shadow)",
            padding: 6,
          }}
        >
          {items.map((it) => (
            <button
              key={it.id}
              role="menuitem"
              disabled={it.disabled}
              title={it.hint}
              style={{
                display: "block",
                width: "100%",
                textAlign: "left" as const,
                border: "none",
                background: "transparent",
                color: it.danger ? "var(--danger)" : "var(--fg)",
                padding: "8px 10px",
                borderRadius: 6,
                fontSize: 13,
                cursor: it.disabled ? "not-allowed" : "pointer",
              }}
              onClick={() => {
                it.onClick();
                onOpenChange(false);
              }}
              onMouseEnter={(e) => {
                if (!it.disabled) e.currentTarget.style.background = "var(--row-hover)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "transparent";
              }}
            >
              {it.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
