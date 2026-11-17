import type { Strings } from "../i18n";
import type { InstalledApp } from "../types";
import { appKey } from "./appKey";

export type UninstallRescan =
  | { phase: "not_started" | "reboot" | "scanning" | "cancelled" | "failed" | "incomplete" }
  | { phase: "complete"; count: number };

export type UninstallReview = { app: InstalledApp; receipt?: number; rescan: UninstallRescan };

export function reviewForApp(review: UninstallReview | null, app: InstalledApp | null | undefined) {
  return review && app && appKey(review.app) === appKey(app) && review.app.version === app.version &&
    review.app.install_location === app.install_location ? review : null;
}

export function uninstallRescanText(L: Strings, rescan: UninstallRescan | undefined, hasReport: boolean): string {
  switch (rescan?.phase) {
    case "reboot": return L.backendRebootInitiated;
    case "scanning": return hasReport ? L.cleanupRescanRunning : L.analyzing;
    case "cancelled": return L.uninstallRescanCancelled;
    case "failed": return L.uninstallRescanFailed;
    case "incomplete": return L.uninstallRescanIncomplete;
    case "complete": return hasReport || rescan.count === 0
      ? L.cleanupRescanResult(rescan.count) : L.uninstallAwaitCleanup(rescan.count);
    default: return L.cleanupRescanUnknown;
  }
}
