/** Rescan after official uninstall (FN-04). Default ON; user can turn off. */
const KEY = "remova_rescan_after_uninstall";

export function loadRescanAfterUninstall(): boolean {
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

export function saveRescanAfterUninstall(on: boolean): void {
  try {
    localStorage.setItem(KEY, on ? "1" : "0");
  } catch {
    /* ignore */
  }
}
