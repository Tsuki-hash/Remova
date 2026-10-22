import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { api } from "../lib/api";

/**
 * IPC contract smoke: every public api method must be present and map to the
 * snake/camel command names the Rust side registers. The command list is parsed
 * straight out of `src-tauri/src/lib.rs` so Rust-side drift — a
 * renamed or newly registered command — fails here instead of silently
 * desyncing from ARCHITECTURE §3.
 */
const COMMAND_KEYS = [
  "listApps",
  "analyze",
  "beginAssociationScan",
  "cancelAssociationScan",
  "associationScanProgress",
  "dryRun",
  "fullCleanup",
  "officialUninstall",
  "verifyLeftovers",
  "orphanScan",
  "rankIdleApps",
  "brokenInstalls",
  "cleanupBrokenInstall",
  "firewallOrphans",
  "deleteFirewallRules",
  "diagnoseLockedPath",
  "endLockingProcess",
  "deepRegistryCheck",
  "scanInstallerCaches",
  "scanToolCaches",
  "listTopDirSizes",
  "listDirChildren",
  "listLocalDrives",
  "isElevated",
  "elevateRestart",
  "openPath",
  "checkGithubLatest",
  "onlineUpdateSupported",
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
  "previewRestore",
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
      [...api.matchAll(/invoke(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g)].map((m) => m[1] ?? "").filter(Boolean),
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
 // real cross-check — a command registered on the Rust side with no
 // caller, or an invoke() targeting an unregistered command, fails here.
 // backendOnly is parsed from check-commands.ps1 (single source).
    const backendOnly = parseBackendOnlyFromPs1();
    const registered = rustRegisteredCommands().filter((c) => !backendOnly.includes(c));
    expect(feInvokedCommands()).toEqual(registered);
  });
});

/** extract `$backendOnly` names from scripts/check-commands.ps1. */
function parseBackendOnlyFromPs1(): string[] {
  const ps1 = readFileSync("scripts/check-commands.ps1", "utf8");
  const m = ps1.match(/foreach \(\$n in @\(([^)]+)\)\)/);
  const list = m?.[1];
  if (!list) throw new Error("check-commands.ps1: $backendOnly list not found");
  return list
    .split(",")
    .map((s) => s.trim().replace(/^'|'$/g, ""))
    .filter(Boolean);
}

/**
 * CQ-04: `ai_risk_brief` / `ai_summarize_report` used to take
 * `Record<string, unknown>` — the only IPC payload in the project that bypassed
 * the type system, so a Rust-side field rename surfaced only at runtime as a
 * silently missing JSON key. These two checks parse both sides and fail on drift.
 */

/** snake_case → camelCase, matching `#[serde(rename_all = "camelCase")]`. */
function camel(snake: string): string {
  return snake.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());
}

/** Field names of a struct in `src-tauri/src/ai.rs`, as serde serializes them. */
function rustAiStructFields(structName: string): string[] {
  const src = readFileSync(new URL("../../src-tauri/src/ai.rs", import.meta.url), "utf8");
  const start = src.indexOf(`pub struct ${structName} {`);
  expect(start, `ai.rs: struct ${structName} not found`).toBeGreaterThan(-1);
  const end = src.indexOf("\n}", start);
  const body = src.slice(start, end);
  const fields = [...body.matchAll(/\bpub ([a-z0-9_]+)\s*:/g)].map((m) => camel(m[1]!));
  expect(fields.length, `ai.rs: ${structName} has no fields`).toBeGreaterThan(0);
  return fields.sort();
}

/** Field names of an exported type in `src/types.ts`. */
function tsTypeFields(typeName: string): string[] {
  const src = readFileSync(new URL("../types.ts", import.meta.url), "utf8");
  const start = src.indexOf(`export type ${typeName} = {`);
  expect(start, `types.ts: type ${typeName} not found`).toBeGreaterThan(-1);
  const end = src.indexOf("\n};", start);
  const body = src.slice(start, end);
  const fields = [...body.matchAll(/^\s{2}([A-Za-z_][A-Za-z0-9_]*)\??:/gm)].map((m) => m[1]!);
  expect(fields.length, `types.ts: ${typeName} has no fields`).toBeGreaterThan(0);
  return fields.sort();
}

describe("AI request payload contract (CQ-04)", () => {
  it.each([
    ["RiskBriefRequest", "RiskBriefInput"],
    ["ReportBriefRequest", "ReportBriefInput"],
  ] as const)("%s field names match ai.rs::%s", (tsName, rustName) => {
    expect(tsTypeFields(tsName)).toEqual(rustAiStructFields(rustName));
  });

  it("no longer accepts an untyped IPC body for the AI commands", () => {
    const src = readFileSync(new URL("../lib/api.ts", import.meta.url), "utf8");
    const brief = src.slice(src.indexOf("aiRiskBrief:"), src.indexOf("aiExplain:"));
    const summarize = src.slice(src.indexOf("aiSummarizeReport:"), src.indexOf("aiParseIntent:"));
    for (const body of [brief, summarize]) {
      expect(body).not.toContain("Record<string, unknown>");
      expect(body).toMatch(/request: (RiskBriefRequest|ReportBriefRequest)/);
    }
  });

  it("keeps the full-cleanup report fields the UI consumes", () => {
    const types = readFileSync("src/types.ts", "utf8");
    const rust = readFileSync("src-tauri/src/executor/mod.rs", "utf8");
    expect(types).toContain("reboot_pending");
    expect(rust).toContain("pub reboot_pending: bool");
    expect(types).toContain("uninstall_ok");
    expect(rust).toContain("pub uninstall_ok: bool");
  });
});
