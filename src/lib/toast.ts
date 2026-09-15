/**
 * Lightweight toast store. Mount ToastHost once; call toast.* anywhere.
 */
export type ToastKind = "success" | "error" | "info";

export type ToastItem = {
  id: number;
  kind: ToastKind;
  message: string;
  sticky?: boolean;
  detail?: string;
};

type Listener = () => void;

let items: ToastItem[] = [];
let seq = 1;
const listeners = new Set<Listener>();
const timers = new Map<number, ReturnType<typeof setTimeout>>();

function emit() {
  for (const l of listeners) l();
}

function push(kind: ToastKind, message: string, opts?: { sticky?: boolean; detail?: string }) {
  const id = seq++;
  const item: ToastItem = {
    id,
    kind,
    message,
    sticky: opts?.sticky,
    detail: opts?.detail,
  };
  items = [...items, item].slice(-5);
  if (!item.sticky) {
    timers.set(
      id,
      setTimeout(() => dismissToast(id), kind === "error" ? 6000 : 4000),
    );
  }
  emit();
  return id;
}

export function dismissToast(id: number) {
  const t = timers.get(id);
  if (t) {
    clearTimeout(t);
    timers.delete(id);
  }
  items = items.filter((i) => i.id !== id);
  emit();
}

export const toast = {
  success(message: string, opts?: { sticky?: boolean; detail?: string }) {
    return push("success", message, opts);
  },
  error(message: string, opts?: { sticky?: boolean; detail?: string }) {
    return push("error", message, { sticky: true, ...opts });
  },
  info(message: string, opts?: { sticky?: boolean; detail?: string }) {
    return push("info", message, opts);
  },
  dismiss(id: number) {
    dismissToast(id);
  },
  clear() {
    for (const id of [...timers.keys()]) clearTimeout(timers.get(id)!);
    timers.clear();
    items = [];
    emit();
  },
};

export function getToasts(): ToastItem[] {
  return items;
}

export function subscribeToasts(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
