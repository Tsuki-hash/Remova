// Share an exclusive update slot with every native operation (including restore
// and service management, which do not all use the software page's busyRef).
let active = 0;
let updating = false;

export async function trackNativeCall<T>(call: () => Promise<T>): Promise<T> {
  if (updating) throw new Error("update:busy");
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
