/** Stable row/multi-select key (avoid ambiguous string concat). */
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

/** Resolve a cleanup report back to its analyzed app; app_key wins over name. */
export function findReportApp<T extends InstalledApp>(
  report: { app_name: string; app_key?: string },
  apps: readonly T[],
  selected: T | null | undefined,
): T | undefined {
  const matches = (a: T) => !report.app_key || appKey(a) === report.app_key;
  return apps.find(matches) ?? (selected && matches(selected) ? selected : undefined);
}
