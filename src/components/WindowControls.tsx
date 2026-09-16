import { useCallback, useEffect, useState } from "react";
import { t } from "../i18n";
import { toast } from "../lib/toast";
import { markQuitIntent, resolveCloseAction } from "../lib/closeMode";

const btnStyle: React.CSSProperties = {
  width: 40,
  height: 32,
  border: "none",
  background: "transparent",
  color: "var(--fg)",
  cursor: "pointer",
  display: "grid",
  placeItems: "center",
  fontSize: 14,
  lineHeight: 1,
  borderRadius: 6,
  padding: 0,
};

/** Custom window controls for the undecorated shell. */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  const L = t();

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        setMaximized(await win.isMaximized());
        const un = await win.onResized(async () => {
          try {
            setMaximized(await win.isMaximized());
          } catch {
            // ignore
          }
        });
        if (cancelled) un();
        else unlisten = un;
      } catch {
        // not in tauri
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Keep drag-region header from stealing control clicks.
  const stopDrag = (e: React.SyntheticEvent) => {
    e.stopPropagation();
  };

  const minimize = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().minimize();
    } catch (err) {
      console.error("minimize failed", err);
    }
  }, []);

  const toggleMax = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().toggleMaximize();
    } catch (err) {
      console.error("toggleMaximize failed", err);
    }
  }, []);

  /**
   * Tray → hide immediately (no CloseRequested).
   * Quit → mark intent then close(); onCloseRequested destroy()s without re-asking.
   */
  const close = useCallback(
    async (e: React.MouseEvent) => {
      e.stopPropagation();
      try {
        const action = await resolveCloseAction();
        if (action === "cancel") return;
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        if (action === "tray") {
          await win.hide();
          toast.info(L.closeToTrayHint);
          return;
        }
        markQuitIntent();
        await win.close();
      } catch (err) {
        console.error("close failed", err);
      }
    },
    [L.closeToTrayHint],
  );

  return (
    <div
      style={{ display: "flex", alignItems: "center", gap: 2, marginLeft: 4 }}
      onMouseDown={stopDrag}
      onMouseUp={stopDrag}
      onClick={stopDrag}
      onDoubleClick={stopDrag}
    >
      <button
        type="button"
        aria-label={L.winMinimize}
        title={L.winMinimize}
        style={btnStyle}
        onMouseDown={stopDrag}
        onClick={(e) => void minimize(e)}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
          <rect x="1" y="4.5" width="8" height="1" fill="currentColor" />
        </svg>
      </button>
      <button
        type="button"
        aria-label={maximized ? L.winRestore : L.winMaximize}
        title={maximized ? L.winRestore : L.winMaximize}
        style={btnStyle}
        onMouseDown={stopDrag}
        onClick={(e) => void toggleMax(e)}
      >
        {maximized ? (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
            <rect x="2.5" y="0.5" width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1" />
            <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1" />
          </svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
            <rect x="1" y="1" width="8" height="8" fill="none" stroke="currentColor" strokeWidth="1" />
          </svg>
        )}
      </button>
      <button
        type="button"
        aria-label={L.winClose}
        title={L.winClose}
        style={btnStyle}
        onMouseDown={stopDrag}
        onClick={(e) => void close(e)}
        onMouseEnter={(e) => {
          e.currentTarget.style.background = "#E81123";
          e.currentTarget.style.color = "#fff";
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.background = "transparent";
          e.currentTarget.style.color = "var(--fg)";
        }}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
          <path d="M1 1l8 8M9 1L1 9" stroke="currentColor" strokeWidth="1.2" />
        </svg>
      </button>
    </div>
  );
}
