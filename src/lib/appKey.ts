/** Stable row/multi-select key (CODE-8: avoid ambiguous string concat). */
import type { InstalledApp, ScanResult } from "../types";

const SEP = String.fromCharCode(0);

export function appKey(a: InstalledApp): string {
  return [a.source, a.registry_key, a.name].join(SEP);
}

/** Does this scan belong to this app? Prefers the backend-built app_key —
 * two same-named apps from different sources must not cross-pair. */
export function scanMatchesApp(scan: Pick<ScanResult, "app_name" | "app_key">, app: InstalledApp): boolean {
  return scan.app_key ? scan.app_key === appKey(app) : scan.app_name === app.name;
}
