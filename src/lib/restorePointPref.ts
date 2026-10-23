/**
 * Windows restore point before a real cleanup. Default ON, matching the Rust
 * `#[serde(default = "default_true")]` on `FullCleanupOptions::restore_point`.
 *
 * the field existed only on the Rust side, so every real cleanup requested
 * a restore point and the user had no way to decline. That mattered because
 * Windows rate-limits restore points to roughly one per 24 hours per machine:
 * once the limit is hit, every later cleanup logs a failure — and before the
 * status code was surfaced, that failure was indistinguishable from "System
 * Restore is turned off", which is what the UI claimed.
 */
const KEY = "remova_restore_point";

export function loadRestorePointPref(): boolean {
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

export function saveRestorePointPref(on: boolean): void {
  try {
    localStorage.setItem(KEY, on ? "1" : "0");
  } catch {
    /* ignore */
  }
}
