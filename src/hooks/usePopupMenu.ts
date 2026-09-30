import { useEffect, type RefObject } from "react";

/**
 * shared popup-menu focus behavior (single source for the
 * AppDetailPanel and AppRow row menus).
 * - opening lands focus on the first `role="menuitem"`;
 * - ArrowDown/ArrowUp/Home/End roam the items;
 * - Escape closes and returns focus to the trigger;
 * - mousedown outside the menu (and outside the trigger) closes without
 *   stealing focus.
 */
export function usePopupMenu({
  open,
  menuRef,
  triggerRef,
  onClose,
}: {
  open: boolean;
  menuRef: RefObject<HTMLElement | null>;
  triggerRef: RefObject<HTMLElement | null>;
  onClose: (restoreFocus: boolean) => void;
}) {
  useEffect(() => {
    if (!open) return;
    const items = (): HTMLButtonElement[] =>
      Array.from(
        menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [],
      );
    items()[0]?.focus();
    const onDoc = (e: MouseEvent) => {
      if (
        menuRef.current?.contains(e.target as Node) ||
        triggerRef.current?.contains(e.target as Node)
      ) {
        return;
      }
      onClose(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === "Tab") {
        // Tab must not leave focus outside an open menu — close and restore.
        e.preventDefault();
        onClose(true);
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Home" || e.key === "End") {
        const list = items();
        if (list.length === 0) return;
        e.preventDefault();
        const idx = list.indexOf(document.activeElement as HTMLButtonElement);
        const next =
          e.key === "ArrowDown"
            ? (idx + 1) % list.length
            : e.key === "ArrowUp"
              ? (idx - 1 + list.length) % list.length
              : e.key === "Home"
                ? 0
                : list.length - 1;
        list[next]?.focus();
      }
    };
    document.addEventListener("mousedown", onDoc);
    window.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("keydown", onKey);
    };
    // The effect owns the open-lifecycle only; onClose is captured at open time.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, menuRef, triggerRef]);
}
