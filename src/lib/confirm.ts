/**
 * Promise-based confirm dialog store.
 * UI mounts ConfirmHost once; callers `await requestConfirm(...)`.
 */
export type ConfirmOptions = {
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  /** Hold duration for danger confirm (ms). 0 = single click. */
  holdMs?: number;
};

type Listener = () => void;

let current: ConfirmOptions | null = null;
let resolver: ((ok: boolean) => void) | null = null;
const listeners = new Set<Listener>();

function emit() {
  for (const l of listeners) l();
}

export function requestConfirm(opts: ConfirmOptions): Promise<boolean> {
  if (resolver) {
    // Replace any pending dialog: previous request is cancelled.
    const prev = resolver;
    resolver = null;
    prev(false);
  }
  return new Promise((resolve) => {
    current = opts;
    resolver = resolve;
    emit();
  });
}

export function settleConfirm(ok: boolean) {
  const r = resolver;
  current = null;
  resolver = null;
  emit();
  r?.(ok);
}

export function getConfirm(): ConfirmOptions | null {
  return current;
}

export function subscribeConfirm(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
