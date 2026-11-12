import { useLayoutEffect, useRef, type RefObject } from "react";

export const dialogLayer = {
  popup: 8000,
  overview: 8500,
  confirm: 8600,
  closeChoice: 8700,
  update: 10000,
};

const FOCUSABLE =
  'button, [href], input:not([type="hidden"]), select, textarea, [tabindex]';

type Dialog = {
  element: HTMLElement;
  layer: number;
  previousFocus: HTMLElement | null;
  escape: () => void;
};

const dialogs: Dialog[] = [];
const popupMenus = new Map<HTMLElement, HTMLElement | null>();
let pageFocus: HTMLElement | null = null;

function topDialog() {
  return dialogs.reduce<Dialog | undefined>((top, dialog) =>
    !top || dialog.layer >= top.layer ? dialog : top, undefined);
}

function focusableItems(element: HTMLElement) {
  return Array.from(element.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (item) => item.tabIndex >= 0 && !item.matches(":disabled") &&
      !item.closest('[hidden], [inert], [aria-hidden="true"]'),
  );
}

function focusDialog(dialog: Dialog, preferred?: HTMLElement | null) {
  const items = focusableItems(dialog.element);
  const target = preferred && items.includes(preferred) ? preferred : items[0] ?? dialog.element;
  target.focus();
}

function handleKeyDown(event: KeyboardEvent) {
  const top = topDialog();
  if (!top) return;
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    if (!event.repeat) top.escape();
    return;
  }
  if (event.key !== "Tab") return;
  const items = focusableItems(top.element);
  const index = items.findIndex((item) => item === document.activeElement);
  if (index < 0 || (event.shiftKey ? index === 0 : index === items.length - 1)) {
    event.preventDefault();
    const target = event.shiftKey ? items.at(-1) : items[0];
    (target ?? top.element).focus();
  }
}

function containFocus(event: FocusEvent) {
  const top = topDialog();
  if (top && event.target instanceof Node && !top.element.contains(event.target)) {
    focusDialog(top);
  }
}

function setCovered(element: HTMLElement, covered: boolean) {
  element.toggleAttribute("inert", covered);
  if (covered) element.setAttribute("aria-hidden", "true");
  else element.removeAttribute("aria-hidden");
}

function updatePopupMenus() {
  const top = topDialog();
  for (const [menu, trigger] of popupMenus) {
    setCovered(menu, Boolean(top && !top.element.contains(menu) && !top.element.contains(trigger)));
  }
}

export function registerPopupMenu(menu: HTMLElement, trigger: HTMLElement | null) {
  popupMenus.set(menu, trigger);
  updatePopupMenus();
  return () => {
    popupMenus.delete(menu);
    setCovered(menu, false);
  };
}

export function useDialogFocus(
  open: boolean,
  containerRef: RefObject<HTMLElement | null>,
  layer: number,
  onEscape?: () => void,
) {
  const escape = useRef(onEscape);
  useLayoutEffect(() => { escape.current = onEscape; }, [onEscape]);

  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!open || !element) return;
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const previous = topDialog();
    if (!previous) {
      pageFocus = previousFocus;
      window.addEventListener("keydown", handleKeyDown, true);
      document.addEventListener("focusin", containFocus);
    }
    const dialog = { element, layer, previousFocus, escape: () => escape.current?.() };
    dialogs.push(dialog);
    if (topDialog() === dialog) {
      focusDialog(dialog);
      if (previous) setCovered(previous.element, true);
    } else {
      setCovered(dialog.element, true);
    }
    updatePopupMenus();

    return () => {
      const wasTop = topDialog() === dialog;
      dialogs.splice(dialogs.indexOf(dialog), 1);
      setCovered(dialog.element, false);
      if (!wasTop) return;
      const next = topDialog();
      updatePopupMenus();
      if (next) {
        setCovered(next.element, false);
        focusDialog(next, dialog.previousFocus);
      } else {
        window.removeEventListener("keydown", handleKeyDown, true);
        document.removeEventListener("focusin", containFocus);
        if (pageFocus?.isConnected) pageFocus.focus();
        pageFocus = null;
      }
    };
  }, [open, containerRef, layer]);
}
