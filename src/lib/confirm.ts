/**
 * Promise-based confirm dialog store.
 * UI mounts ConfirmHost once; callers `await requestConfirm(...)`.
 */
export type ConfirmCheckbox = {
  label: string;
  defaultChecked?: boolean;
};

export type ConfirmOptions = {
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  /** Hold duration for danger confirm (ms). 0 = single click. */
  holdMs?: number;
  /** Optional opt-in checkbox (e.g. backup before cleanup). */
  checkbox?: ConfirmCheckbox;
};

export type ConfirmResult = { ok: boolean; checked: boolean };

type Listener = () => void;

let current: ConfirmOptions | null = null;
let currentChecked = false;
let resolver: ((r: ConfirmResult) => void) | null = null;
const listeners = new Set<Listener>();

function emit() {
  for (const l of listeners) l();
}

function publish(opts: ConfirmOptions | null): Promise<ConfirmResult> {
  if (resolver) {
    const prev = resolver;
    resolver = null;
    prev({ ok: false, checked: false });
  }
  currentChecked = opts?.checkbox?.defaultChecked ?? false;
  return new Promise((resolve) => {
    current = opts;
    resolver = resolve;
    emit();
  });
}

/** Resolve confirm true/false only (checkbox ignored). */
export function requestConfirm(opts: ConfirmOptions): Promise<boolean> {
  return requestConfirmEx(opts).then((r) => r.ok);
}

/** Resolve confirm + checkbox state (default unchecked unless defaultChecked). */
export function requestConfirmEx(opts: ConfirmOptions): Promise<ConfirmResult> {
  return publish(opts);
}

export function setConfirmChecked(v: boolean) {
  currentChecked = v;
}

export function getConfirmChecked(): boolean {
  return currentChecked;
}

export function settleConfirm(ok: boolean) {
  const checked = ok ? currentChecked : false;
  const r = resolver;
  current = null;
  resolver = null;
  emit();
  r?.({ ok, checked });
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
