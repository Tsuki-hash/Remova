/** Close-to-tray vs quit policy (persisted locally). */

export type CloseMode = "tray" | "quit";
export type CloseChoice = CloseMode | "cancel";

const KEY = "remova_close_mode";
let quitIntent = false;

/** null = user has not saved a preference yet (ask on close). */
export function loadCloseMode(): CloseMode | null {
  try {
    const v = localStorage.getItem(KEY);
    return v === "quit" ? "quit" : v === "tray" ? "tray" : null;
  } catch {
    return null;
  }
}

export function saveCloseMode(mode: CloseMode) {
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    // ignore
  }
}

export function clearCloseMode() {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // ignore
  }
}

/** X / menu already chose quit — onCloseRequested must destroy, not re-ask. */
export function markQuitIntent() {
  quitIntent = true;
}

export function consumeQuitIntent(): boolean {
  const v = quitIntent;
  quitIntent = false;
  return v;
}

// --- first-close choice dialog store ---

type Listener = () => void;
let dialogOpen = false;
let resolver: ((c: CloseChoice) => void) | null = null;
const listeners = new Set<Listener>();

function emit() {
  for (const l of listeners) l();
}

export function isCloseChoiceOpen() {
  return dialogOpen;
}

export function subscribeCloseChoice(l: Listener) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

export function requestCloseChoice(): Promise<CloseChoice> {
  if (resolver) {
    const prev = resolver;
    resolver = null;
    prev("cancel");
  }
  return new Promise((resolve) => {
    dialogOpen = true;
    resolver = resolve;
    emit();
  });
}

export function settleCloseChoice(choice: CloseChoice) {
  const r = resolver;
  dialogOpen = false;
  resolver = null;
  emit();
  r?.(choice);
}

/** Saved preference, or first-time dialog. Returns the action to run. */
export async function resolveCloseAction(): Promise<CloseMode | "cancel"> {
  const saved = loadCloseMode();
  if (saved) return saved;
  return requestCloseChoice();
}
