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
    startup: L.linkedStartup, other: L.linkedOther };
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
      ["installer package", L.installerTitle], ["updater cache", L.linkedConfigFiles],
      ["tool cache (", L.toolcacheTitle], ["Orphan app-like folder", L.orphanScan],
      ["Install monitor: new registry entry", L.linkedRegistry],
      ["Install monitor: cache/log-like path", L.linkedConfigFiles], ["Install monitor: new path", L.monitorDiff]];
    const rule = rules.find(([prefix]) => raw.startsWith(prefix));
    return rule ? rule[1] : L.backendDetailUnavailable;
  }
  const exact: Record<string, string> = {
    skipped: L.backendOfficialSkipped, "no items": L.backendNoItems,
    "no uninstall string": L.uninstallNoCmd,
    "uninstall command not from latest app scan": L.backendScanExpired,
    "uninstall launch failed": L.backendLaunchFailed, "uninstall wait failed": L.backendWaitFailed,
    "backup session failed": L.backendBackupFailed, "backup failed": L.backendBackupFailed,
    "dry-run: official uninstaller not launched": L.backendDryRun,
    "[dry-run]": L.backendDryRun, "PATH entry removed": L.backendPathRemoved,
    "reparse point": L.backendReparse,
    "SRSetRestorePointW failed": L.restorePointFail, "not windows": L.backendUnsupported,
  };
  if (Object.hasOwn(exact, raw)) return exact[raw]!;
  if (raw.startsWith("uninstaller finished: ")) return L.uninstallOk;
  if (raw.startsWith("uninstaller timed out (5m): ")) return L.backendTimeout;
  if (raw.startsWith("uninstaller exited with ")) return L.backendUninstallExit(raw.match(/Some\((-?\d+)\)/)?.[1] ?? "?");
  if (raw.startsWith("backup failed for ")) return L.backendBackupFailed;
  if (raw.startsWith("restore point seq=")) return L.restorePointOk;
  if (/^(sc|schtasks) delete .+: ok$/.test(raw)) return L.backendNativeDeleted;
  if (/^(sc|schtasks) delete .+: failed or not found$/.test(raw)) return L.backendNativeDeleteFailed;
  return L.backendDetailUnavailable;
}
