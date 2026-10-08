import { useSyncExternalStore } from "react";

type Request = { id: number; name: string };
let current: Request | null = null;
let next = 0;
const listeners = new Set<() => void>();
const publish = () => { for (const listener of listeners) listener(); };
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };

/** Ephemeral navigation intent, never a restore or delete authorization. */
export function requestBackupSession(name: string) {
  current = { id: ++next, name };
  publish();
}
export function clearBackupSessionRequest(id: number) {
  if (current?.id === id) { current = null; publish(); }
}
export function useBackupSessionRequest() {
  return useSyncExternalStore(subscribe, () => current);
}
