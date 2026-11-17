import type { Strings } from "../i18n";
import type { InstalledApp } from "../types";
import { appKey } from "./appKey";
import { uninstallNeedsRestart } from "./backendText";

export type UninstallRescan =
  | { phase: "not_started" | "reboot" | "scanning" | "cancelled" | "failed" | "incomplete" | "expired" | "installed" | "unverified" }
  | { phase: "complete"; count: number };

export type UninstallReview = {
  app: InstalledApp;
  receipt?: number;
  rescan: UninstallRescan;
  restart?: "required" | "before_verification";
};

export function sameReviewApp(a: InstalledApp, b: InstalledApp) {
  return appKey(a) === appKey(b) && a.version === b.version && a.install_location === b.install_location;
}

export function uninstallReviewFromResult(
  app: InstalledApp,
  { ok, message, reboot, receipt }: { ok: boolean; message: string; reboot?: boolean; receipt?: number },
): UninstallReview | undefined {
  const needsRestart = !reboot && uninstallNeedsRestart(message);
  if (!ok && !needsRestart) return undefined;
  return {
    app,
    ...(receipt !== undefined && ok && !reboot && { receipt }),
    ...(needsRestart && { restart: ok ? "required" : "before_verification" }),
    rescan: reboot ? { phase: "reboot" } : message === "uninstall review could not verify installation"
      ? { phase: "unverified" } : { phase: "not_started" },
  };
}

export function rescanFailure(error: unknown): UninstallRescan {
  switch (error instanceof Error ? error.message : error) {
    case "scan:cancelled": return { phase: "cancelled" };
    case "uninstall review expired": return { phase: "expired" };
    case "uninstall review found an installed application": return { phase: "installed" };
    case "uninstall review could not verify installation": return { phase: "unverified" };
    default: return { phase: "failed" };
  }
}

export function canRescanReview(review: UninstallReview): boolean {
  return review.receipt !== undefined &&
    !["reboot", "scanning", "expired", "installed"].includes(review.rescan.phase);
}

export function uninstallRescanText(L: Strings, rescan: UninstallRescan | undefined, hasReport: boolean): string {
  switch (rescan?.phase) {
    case "reboot": return L.backendRebootInitiated;
    case "scanning": return hasReport ? L.cleanupRescanRunning : L.analyzing;
    case "cancelled": return L.uninstallRescanCancelled;
    case "failed": return L.uninstallRescanFailed;
    case "unverified": return L.uninstallReviewUnverified;
    case "expired": return L.uninstallReviewExpired;
    case "installed": return L.uninstallReviewInstalled;
    case "incomplete": return L.uninstallRescanIncomplete;
    case "complete": return hasReport || rescan.count === 0
      ? L.cleanupRescanResult(rescan.count) : L.uninstallAwaitCleanup(rescan.count);
    default: return L.cleanupRescanUnknown;
  }
}
