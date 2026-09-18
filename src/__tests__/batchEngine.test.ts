import { beforeEach, describe, expect, it, vi } from "vitest";
import type { InstalledApp, ScanResult, FullCleanupReport, CleanupItem } from "../types";

const analyze = vi.fn();
const fullCleanup = vi.fn();

vi.mock("../lib/api", () => ({
  api: {
    analyze: (...a: unknown[]) => analyze(...a),
    fullCleanup: (...a: unknown[]) => fullCleanup(...a),
  },
}));

vi.mock("../i18n", () => ({
  t: () => ({
    selectRowHint: "select",
    toastBatchProgress: (i: number, n: number, name: string) => `${i}/${n} ${name}`,
    batchDetail: (d: number, f: number) => `del${d} fail${f}`,
    batchCancelled: "cancelled",
    batchDone: "done",
  }),
}));

vi.mock("../lib/toast", () => ({
  toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() },
}));

vi.mock("../lib/format", () => ({
  formatError: (e: unknown, _ctx?: string) => String(e),
  prettyAppName: (n: string, _s?: string) => n,
}));

import { runBatchCleanup } from "../lib/batchEngine";

function app(name: string, key = name): InstalledApp {
  return {
    name,
    version: "1",
    publisher: "P",
    install_location: `C:\\Program Files\\${name}`,
    uninstall_string: "msiexec /x {GUID}",
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: `Uninstall\\${key}`,
    estimated_size_kb: 1024,
    install_date: "",
    display_icon: "",
  };
}

function item(path: string, partial: Partial<CleanupItem> = {}): CleanupItem {
  return {
    path,
    kind: "dir",
    score: 90,
    confidence: "confirmed",
    risk: "low",
    reason: "t",
    evidence: [],
    shared: false,
    user_data: false,
    ...partial,
  };
}

function scan(appName: string, items: CleanupItem[]): ScanResult {
  return { app_name: appName, items };
}

function report(partial: Partial<FullCleanupReport> = {}): FullCleanupReport {
  return {
    app_name: "X",
    dry_run: false,
    backup_dir: "C:\\bak",
    uninstall_ok: true,
    uninstall_message: "ok",
    deleted: 1,
    failed: 0,
    skipped: 0,
    aborted: false,
    restore_point_ok: true,
    restore_point_msg: "",
    errors: [],
    item_details: [],
    ...partial,
  };
}

function makeCb() {
  return {
    onIndex: vi.fn(),
    onCurrent: vi.fn(),
    onResults: vi.fn(),
    onShowSummary: vi.fn(),
    onSetBatching: vi.fn(),
    onDoneKeys: vi.fn(),
    cancelRef: { current: false },
    busyRef: { current: false },
  };
}

const keyOf = (a: InstalledApp) => a.registry_key;

beforeEach(() => {
  analyze.mockReset();
  fullCleanup.mockReset();
});

describe("runBatchCleanup", () => {
  it("marks ok when leftovers cleaned", async () => {
    analyze.mockResolvedValue(scan("A", [item("C:\\Program Files\\A")]));
    fullCleanup.mockResolvedValue(report({ deleted: 1, failed: 0 }));
    const cb = makeCb();
    await runBatchCleanup([app("A")], true, keyOf, cb);
    expect(cb.onDoneKeys).toHaveBeenCalledWith(["Uninstall\\A"]);
    expect(cb.onShowSummary).toHaveBeenCalledWith(true);
    expect(cb.busyRef.current).toBe(false);
  });

  it("still runs official uninstall when no default-selectable leftovers (F-1)", async () => {
    analyze.mockResolvedValue(
      scan("B", [item("C:\\x", { confidence: "suspected", risk: "medium" })]),
    );
    fullCleanup.mockResolvedValue(
      report({ deleted: 0, failed: 0, uninstall_ok: true, aborted: false }),
    );
    const cb = makeCb();
    await runBatchCleanup([app("B")], true, keyOf, cb);
    expect(fullCleanup).toHaveBeenCalledWith(
      expect.anything(),
      [],
      expect.objectContaining({ dry_run: false, skip_official_uninstall: false }),
    );
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("ok");
  });

  it("records failed when all deletes fail", async () => {
    analyze.mockResolvedValue(scan("C", [item("C:\\Program Files\\C")]));
    fullCleanup.mockResolvedValue(report({ deleted: 0, failed: 2 }));
    const cb = makeCb();
    await runBatchCleanup([app("C")], true, keyOf, cb);
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("failed");
  });

  it("stops early when cancelRef is set", async () => {
    analyze.mockImplementation(async () => scan("D", []));
    fullCleanup.mockResolvedValue(report({ deleted: 0, uninstall_ok: true }));
    const cb = makeCb();
    cb.onIndex.mockImplementation((i: number) => {
      // cancel after the first real item starts (skip the initial onIndex(0) reset)
      if (i >= 1) cb.cancelRef.current = true;
    });
    await runBatchCleanup([app("D1"), app("D2")], true, keyOf, cb);
    expect(analyze).toHaveBeenCalledTimes(1);
    const lastResults = cb.onResults.mock.calls.at(-1)?.[0] ?? [];
    expect(lastResults.length).toBeLessThanOrEqual(1);
  });

  it("maps invoke errors to failed rows", async () => {
    analyze.mockRejectedValue(new Error("boom"));
    const cb = makeCb();
    await runBatchCleanup([app("E")], true, keyOf, cb);
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("failed");
  });
});
