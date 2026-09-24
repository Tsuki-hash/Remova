import { describe, expect, it } from "vitest";
import { api } from "../lib/api";

/**
 * IPC contract smoke: every public api method must be present and map to the
 * snake/camel command names the Rust side registers. Keep this list in sync
 * when adding commands — a missing wrapper means the UI silently cannot call it.
 */
const COMMAND_KEYS = [
  "listApps",
  "analyze",
  "dryRun",
  "fullCleanup",
  "officialUninstall",
  "verifyLeftovers",
  "orphanScan",
  "rankIdleApps",
  "scanInstallerCaches",
  "scanToolCaches",
  "listTopDirSizes",
  "listDirChildren",
  "listLocalDrives",
  "isElevated",
  "elevateRestart",
  "openPath",
  "checkGithubLatest",
  "getAiConfig",
  "saveAiConfig",
  "aiRiskBrief",
  "aiExplain",
  "aiSummarizeReport",
  "aiParseIntent",
  "history",
  "exportHistoryCsv",
  "deleteHistory",
  "clearHistory",
  "backupSessions",
  "deleteBackupSession",
  "restoreSessionByName",
  "suggestIgnoreRules",
  "applyIgnoreSuggestions",
  "ignorePublisher",
  "ignoreAppName",
  "unignorePublisher",
  "unignoreAppName",
  "loadIgnore",
  "diskUsage",
  "beginSizeEstimate",
  "cancelSizeEstimate",
  "estimateDirSizeKb",
  "takePendingAnalyze",
  "beginInstallMonitor",
  "endInstallMonitor",
  "registerContextMenu",
  "unregisterContextMenu",
  "setStartupEnabled",
  "setServiceStartDisabled",
  "setServiceRunning",
  "setTaskEnabled",
  "listStartupItems",
  "listServices",
  "listScheduledTasks",
  "appIconData",
] as const;

describe("api IPC contract", () => {
  it("exposes every command wrapper", () => {
    for (const key of COMMAND_KEYS) {
      expect(api, `api.${key} missing`).toHaveProperty(key);
      expect(typeof (api as Record<string, unknown>)[key], `api.${key} not fn`).toBe(
        "function",
      );
    }
  });

  it("has no unexpected extra methods (keep in sync with Rust commands)", () => {
    const actual = Object.keys(api).sort();
    const expected = [...COMMAND_KEYS].sort();
    expect(actual).toEqual(expected);
  });
});
