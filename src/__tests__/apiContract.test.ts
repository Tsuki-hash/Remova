import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { api } from "../lib/api";

/**
 * IPC contract smoke: every public api method must be present and map to the
 * snake/camel command names the Rust side registers. The command list is parsed
 * straight out of `src-tauri/src/lib.rs` (REV-QA-10) so Rust-side drift — a
 * renamed or newly registered command — fails here instead of silently
 * desyncing from ARCHITECTURE §3.
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

/**
 * Parse the `generate_handler![...]` command list out of lib.rs (snake_case).
 */
function rustRegisteredCommands(): string[] {
  // Repo-relative via import.meta.url — no node globals needed in the test tsconfig.
  const lib = readFileSync(
    new URL("../../src-tauri/src/lib.rs", import.meta.url),
    "utf8",
  );
  const start = lib.indexOf("generate_handler![");
  expect(start, "generate_handler! not found in lib.rs").toBeGreaterThan(-1);
  const end = lib.indexOf("]", start);
  const body = lib.slice(start + "generate_handler![".length, end);
  return [
    ...new Set(
      body
        .split(",")
        .map((s) => s.trim().split("::").pop()?.trim() ?? "")
        .filter(Boolean),
    ),
  ].sort();
}

/**
 * Every `invoke("command_name")` string in the frontend data layer.
 */
function feInvokedCommands(): string[] {
  const api = readFileSync(new URL("../lib/api.ts", import.meta.url), "utf8");
  return [
    ...new Set(
      [...api.matchAll(/invoke(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g)].map((m) => m[1]),
    ),
  ].sort();
}

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

  it("invoke() targets match the commands Rust actually registers (lib.rs)", () => {
    // REV-QA-10: real cross-check — a command registered on the Rust side with no
    // caller, or an invoke() targeting an unregistered command, fails here.
    // Keep this allowlist in sync with $backendOnly in scripts/check-commands.ps1.
    const backendOnly = ["list_restore_sessions"];
    const registered = rustRegisteredCommands().filter((c) => !backendOnly.includes(c));
    expect(feInvokedCommands()).toEqual(registered);
  });
});
