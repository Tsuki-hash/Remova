import { useEffect, type RefObject } from "react";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Shared dialog focus trap (REV-UX-01): initial focus, Tab/Shift+Tab cycle inside
 * the dialog, restore focus on close. Escape is left to each host (cancel semantics differ).
 */
export function useDialogFocus(open: boolean, containerRef: RefObject<HTMLElement | null>) {
  useEffect(() => {
    if (!open) return;
    const container = containerRef.current;
    if (!container) return;
    const prev = document.activeElement as HTMLElement | null;
    const focusables = () =>
      Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
        (el) => el.getAttribute("aria-hidden") !== "true",
      );
    const first = focusables()[0];
    (first ?? container).focus();

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const items = focusables();
      if (items.length === 0) {
        e.preventDefault();
        container.focus();
        return;
      }
      const firstEl = items[0]!;
      const lastEl = items[items.length - 1]!;
      const active = document.activeElement;
      if (e.shiftKey) {
        if (active === firstEl || !container.contains(active)) {
          e.preventDefault();
          lastEl.focus();
        }
      } else if (active === lastEl || !container.contains(active)) {
        e.preventDefault();
        firstEl.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      if (prev && typeof prev.focus === "function" && document.contains(prev)) {
        prev.focus();
      }
    };
  }, [open, containerRef]);
}
