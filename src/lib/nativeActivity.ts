// Share an exclusive update slot with every native operation (including restore
// and service management, which do not all use the software page's busyRef).
/** Stable token thrown when the update slot is held — callers and the error
 * formatter both match on this exact string (unified IPC error contract). */
export const UPDATE_BUSY = "update:busy";

let active = 0;
let updating = false;

export async function trackNativeCall<T>(call: () => Promise<T>): Promise<T> {
  if (updating) throw new Error(UPDATE_BUSY);
  active++;
  try {
    return await call();
  } finally {
    active--;
  }
}

export function acquireUpdateSlot(): (() => void) | null {
  if (updating || active > 0) return null;
  updating = true;
  return () => { updating = false; };
}
