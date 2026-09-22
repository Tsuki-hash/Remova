import { describe, expect, it } from "vitest";
import { appCoreReducer, initialAppCoreState } from "../hooks/reducers/appCore";
import type { CleanupReport, FullCleanupReport } from "../types";

function fullReport(over: Partial<FullCleanupReport> = {}): FullCleanupReport {
  return {
    app_name: "DemoApp",
    dry_run: false,
    backup_dir: "",
    uninstall_ok: true,
    uninstall_message: "",
    deleted: 3,
    failed: 0,
    skipped: 0,
    delayed: 0,
    aborted: false,
    restore_point_ok: false,
    restore_point_msg: "",
    errors: [],
    item_details: [],
    ...over,
  };
}

function dryReport(): CleanupReport {
  return {
    app_name: "DemoApp",
    dry_run: true,
    uninstall_command: [],
    uninstall_message: "",
    deleted_planned: 2,
    skipped: 0,
    errors: [],
    item_details: [],
  };
}

describe("appCore report reducer (single source)", () => {
  it("derives lastReport from a completed full cleanup", () => {
    const next = appCoreReducer(initialAppCoreState(), {
      type: "report/set",
      value: fullReport(),
    });
    expect(next.report?.app_name).toBe("DemoApp");
    expect(next.lastReport?.deleted).toBe(3);
  });

  it("does not let a dry-run preview overwrite lastReport", () => {
    const withFull = appCoreReducer(initialAppCoreState(), {
      type: "report/set",
      value: fullReport({ app_name: "Cleaned" }),
    });
    const next = appCoreReducer(withFull, { type: "report/set", value: dryReport() });
    expect(next.report?.dry_run).toBe(true);
    expect(next.lastReport?.app_name).toBe("Cleaned");
  });

  it("does not record an aborted cleanup as lastReport", () => {
    const next = appCoreReducer(initialAppCoreState(), {
      type: "report/set",
      value: fullReport({ aborted: true, deleted: 0 }),
    });
    expect(next.report && "aborted" in next.report && next.report.aborted).toBe(true);
    expect(next.lastReport).toBeNull();
  });

  it("clearing the preview keeps the last completed cleanup for export", () => {
    const withFull = appCoreReducer(initialAppCoreState(), {
      type: "report/set",
      value: fullReport(),
    });
    const next = appCoreReducer(withFull, { type: "preview/close" });
    expect(next.report).toBeNull();
    expect(next.lastReport?.deleted).toBe(3);
  });

  it("report/set(null) clears the visible report only", () => {
    const withFull = appCoreReducer(initialAppCoreState(), {
      type: "report/set",
      value: fullReport(),
    });
    const next = appCoreReducer(withFull, { type: "report/set", value: null });
    expect(next.report).toBeNull();
    expect(next.lastReport?.deleted).toBe(3);
  });
});
