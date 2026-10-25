import { t } from "../i18n";

/** Localize backend diagnostics without treating unknown text as success. */
export function backendText(raw: string, context: "cleanup" | "scan" | "idle" = "cleanup"): string {
  const L = t();
  if (!raw) return "";
  if (context === "idle") {
    const labels: Record<string, string> = { idle_install_age: L.idleEvInstallAge,
      idle_dir_mtime: L.idleEvDirMtime, idle_size_partial: L.idleEvSizePartial };
    return Object.hasOwn(labels, raw) ? labels[raw]! : L.backendDetailUnavailable;
  }
  const buckets: Record<string, string> = { programFiles: L.linkedProgramFiles,
    configFiles: L.linkedConfigFiles, registry: L.linkedRegistry, shortcuts: L.linkedShortcuts,
    startup: L.linkedStartup, other: L.linkedOther,
    // Specialty-scan buckets. These are the values `push_*_item` writes, and a
    // missing entry here means the row silently renders as "no localized
    // description" — which is exactly what happened before.
    updater_cache: L.installerTitle, download_pkg: L.installerTitle,
    dev: L.toolcacheDomainDev, game: L.toolcacheDomainGame, browser: L.toolcacheDomainBrowser };
  if (context === "scan") {
    if (Object.hasOwn(buckets, raw)) return buckets[raw]!;
    const rules: Array<[string, string]> = [["Install location", L.linkedProgramFiles],
      ["Product dir under ", L.linkedProgramFiles], ["Other-drive product folder: ", L.linkedProgramFiles],
      ["Software key: ", L.linkedRegistry], ["Uninstall registry key", L.linkedRegistry],
      ["Shell/Classes leftover: ", L.linkedRegistry], ["CLSID leftover: ", L.linkedRegistry],
      ["Driver leftover: ", L.backendDriver], ["Windows service leftover: ", L.backendService],
      ["Scheduled task leftover: ", L.backendTask], ["App Paths: ", L.backendAppPath],
      ["PATH entry (", L.backendPathEntry], ["Shortcut", L.linkedShortcuts],
      ["TEMP name match", L.backendTempMatch], ["AppData folder matching product name", L.linkedConfigFiles],
      ["WebView2 / Electron cache folder", L.linkedConfigFiles], ["Run startup: ", L.linkedStartup],
      ["installer package", L.installerTitle],
      ["possible installer package", L.installerMaybeTitle],
      ["updater cache", L.installerTitle],
      ["shared tool cache", L.toolcacheSharedTitle],
      ["tool cache (", L.toolcacheTitle], ["Orphan app-like folder", L.orphanScan],
      ["Install monitor: new registry entry", L.linkedRegistry],
      ["Install monitor: cache/log-like path", L.linkedConfigFiles], ["Install monitor: new path", L.monitorDiff],
      ["MuiCache trace", L.muicacheReason], ["Home dot-directory", L.homeDotReason]];
    const rule = rules.find(([prefix]) => raw.startsWith(prefix));
    return rule ? rule[1] : L.backendDetailUnavailable;
  }
  const exact: Record<string, string> = {
    skipped: L.backendOfficialSkipped, "no items": L.backendNoItems,
    "no uninstall string": L.uninstallNoCmd,
    "app is not from the latest scan": L.backendAppStale,
    "analysis must run before cleanup": L.backendAnalysisRequired,
    "uninstall command uses a relative executable; refused": L.uninstallRelativeRefused,
    "uninstall command not from latest app scan": L.backendScanExpired,
    "uninstall launch failed": L.backendLaunchFailed, "uninstall wait failed": L.backendWaitFailed,
    "elevated HKCU uninstall refused": L.uninstallElevatedHkcu,
    "elevated HKCU analysis refused": L.elevatedHkcuBlocked,
    "elevated HKCU cleanup refused": L.elevatedHkcuBlocked,
    "backup session failed": L.backendBackupFailed, "backup failed": L.backendBackupFailed,
    "dry-run: official uninstaller not launched": L.backendDryRun,
    "[dry-run]": L.backendDryRun, "PATH entry removed": L.backendPathRemoved,
    "reparse point": L.backendReparse,
    "SRSetRestorePointW failed": L.restorePointFail, "not windows": L.backendUnsupported,
  };
  if (Object.hasOwn(exact, raw)) return exact[raw]!;
  if (raw.startsWith("uninstaller finished (reboot required): ")) return L.backendRebootRequired;
  if (
    raw === "official uninstaller initiated reboot; residual cleanup skipped" ||
    raw === "system reboot pending; residual cleanup postponed"
  )
    return L.backendRebootInitiated;
  if (raw === "system reboot pending; uninstall skipped") return L.backendRebootBlocked;
  if (raw.startsWith("cleanup item not from the latest analysis: ")) {
    return L.backendCleanupItemStale(
      raw.slice("cleanup item not from the latest analysis: ".length),
    );
  }
  if (raw === "cleanup cancelled") return L.batchCancelled;
  if (raw === "cleanup cancelled during backup") return L.backendCleanupCancelledBackup;
  if (raw === "migrate:cancelled") return L.migrateCancelled;
  if (raw.startsWith("not a regenerable layout: ")) {
    return L.backendNotRegenerable(raw.slice("not a regenerable layout: ".length));
  }
  if (raw.startsWith("migration is not in the cutover state")) return L.backendMigrateState;
  if (raw.startsWith("backup failed; npm packages kept: ")) return L.backendNpmBackupFailed;
  if (raw.startsWith("package is no longer listed by the fresh scan: ")) {
    return L.backendNpmStale(raw.slice("package is no longer listed by the fresh scan: ".length));
  }
  if (raw === "npm global folder not found") return L.backendNpmNoRoot;
  if (raw.startsWith("not an admitted npm global package: ")) {
    return L.backendNpmNotAdmitted(raw.slice("not an admitted npm global package: ".length));
  }
  if (raw.startsWith("uninstaller finished: ")) return L.uninstallOk;
  if (raw.startsWith("uninstaller timed out (5m): ")) return L.backendTimeout;
  if (raw.startsWith("uninstaller exited with ")) return L.backendUninstallExit(raw.match(/Some\((-?\d+)\)/)?.[1] ?? "?");
  if (raw.includes("process image changed")) return L.lockImageChanged;
  if (raw.includes("process no longer holds this path")) return L.lockNoLongerHolds;
  if (raw.includes("process did not exit in time")) return L.lockExitTimeout;
  if (raw.includes("process image is outside the install directory")) return L.lockOutsideRoot;
  if (raw.includes("path is not from the latest cleanup")) return L.lockTargetStale;
  if (raw.includes("firewall rule is no longer an orphan")) return L.firewallRuleStale;
  if (raw.includes("broken install record no longer present")) return L.brokenInstallsGone;
  if (raw.startsWith("backup failed for ")) return L.backendBackupFailed;
  if (raw.startsWith("restore point seq=")) return L.restorePointOk;
  if (/^(sc|schtasks) delete .+: ok$/.test(raw)) return L.backendNativeDeleted;
  if (/^(sc|schtasks) delete .+: failed or not found$/.test(raw)) return L.backendNativeDeleteFailed;
  return L.backendDetailUnavailable;
}

/** Localize known backend diagnostics, preserving unknown raw text verbatim. */
export function backendErrorText(raw: string, context: "cleanup" | "scan" = "cleanup"): string {
  const L = t();
  const localized = backendText(raw, context);
  return localized === L.backendDetailUnavailable ? raw : localized;
}
