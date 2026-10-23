import { beforeEach, describe, expect, it, vi } from "vitest";
import type { InstalledApp, ScanResult, FullCleanupReport, CleanupItem } from "../types";

const analyze = vi.fn();
const fullCleanup = vi.fn();
const beginScan = vi.fn(async () => 7);
const cancelScan = vi.fn(async () => true);

vi.mock("../lib/api", () => ({
  api: {
    analyze: (...a: unknown[]) => analyze(...a),
    fullCleanup: (...a: unknown[]) => fullCleanup(...a),
    beginAssociationScan: () => beginScan(),
    cancelAssociationScan: (...a: unknown[]) => cancelScan(...a),
  },
}));

vi.mock("../i18n", () => ({
  t: () => ({
    selectRowHint: "select",
    toastBatchProgress: (i: number, n: number, name: string) => `${i}/${n} ${name}`,
    batchDetail: (d: number, f: number) => `del${d} fail${f}`,
    cleanupOutcome: (d: number, f: number, s: number, p: number) => `del${d} fail${f} skip${s} pend${p}`,
    cleanupNothingRemoved: (s: number) => (s > 0 ? `refused ${s}` : "nothing to clean"),
    batchCancelled: "cancelled",
    batchDone: "done",
    backendBackupFailed: "backup stopped cleanup",
    backendDetailUnavailable: "diagnostic unavailable",
    backendRebootInitiated: "reboot pending",
    uninstallFail: "uninstall failed",
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
  beginScan.mockReset();
  beginScan.mockResolvedValue(7);
  cancelScan.mockReset();
  cancelScan.mockResolvedValue(true);
});

describe("runBatchCleanup", () => {
  it.each([false, true])("keeps an empty residual abort distinct from a read failure (readFailed=%s)", async readFailed => {
    const target = app("EmptyRetry");
    analyze.mockResolvedValue(scan("EmptyRetry", []));
    fullCleanup.mockResolvedValue(report({ aborted: true, uninstall_ok: false, deleted: 0,
      uninstall_message: readFailed ? "ignore read failed" : "no items",
      errors: readFailed ? ["read failed"] : [] }));
    const cb = makeCb();
    await runBatchCleanup([target], true, keyOf, cb, false, new Set([keyOf(target)]));
    expect(cb.onResults.mock.calls.at(-1)?.[0]?.[0]?.status).toBe(readFailed ? "failed" : "skipped");
    expect(cb.onDoneKeys).toHaveBeenCalledWith(readFailed ? [] : [keyOf(target)]);
  });

  it.each([
    { deleted: 1, failed: 1, delayed: 0, aborted: false, status: "failed" },
    { deleted: 0, failed: 1, delayed: 0, aborted: false, status: "failed" },
    { deleted: 0, failed: 0, delayed: 1, aborted: false, status: "ok" },
    { deleted: 0, failed: 0, delayed: 0, aborted: true, status: "failed" },
  ])("classifies residual retry boundaries without repeating official work: %j", async expected => {
    const target = app("Boundary");
    analyze.mockResolvedValue(scan("Boundary", [item("fixture")]));
    fullCleanup.mockResolvedValue(report({ ...expected, uninstall_ok: false }));
    const cb = makeCb();
    await runBatchCleanup([target], true, keyOf, cb, false, new Set([keyOf(target)]));
    expect(cb.onResults.mock.calls.at(-1)?.[0]?.[0]?.status).toBe(expected.status);
    expect(cb.onDoneKeys).toHaveBeenCalledWith(expected.status === "ok" ? [keyOf(target)] : []);
    expect(fullCleanup.mock.calls[0]?.[2]).toMatchObject({ skip_official_uninstall: true });
  });

  it("retains partial failures and retries residue without repeating a successful uninstall", async () => {
    const target = app("Partial");
    analyze.mockResolvedValue(scan("Partial", [item("fixture")]));
    fullCleanup.mockResolvedValueOnce(report({ deleted: 1, failed: 1 }));
    const cb = makeCb();
    await runBatchCleanup([target], true, keyOf, cb);
    expect(cb.onDoneKeys).toHaveBeenCalledWith([]);
    expect(cb.onResults.mock.calls.at(-1)?.[0]?.[0]).toMatchObject({ status: "failed", officialDone: true });
    fullCleanup.mockResolvedValueOnce(report({ deleted: 1, failed: 0, uninstall_ok: false }));
    const retry = makeCb();
    await runBatchCleanup([target], true, keyOf, retry, false, new Set([keyOf(target)]));
    expect(fullCleanup.mock.calls.at(-1)?.[2]).toMatchObject({ skip_official_uninstall: true });
    expect(retry.onDoneKeys).toHaveBeenCalledWith([keyOf(target)]);
    analyze.mockRejectedValueOnce(new Error("retry analyze failed"));
    const later = makeCb();
    await runBatchCleanup([target], true, keyOf, later, false, new Set([keyOf(target)]));
    expect(later.onResults.mock.calls.at(-1)?.[0]?.[0]).toMatchObject({ status: "failed", officialDone: true });
  });

  it.each([true, false])("does not clear selection after an empty official failure (aborted=%s)", async (aborted) => {
    analyze.mockResolvedValue(scan("A", []));
    fullCleanup.mockResolvedValue(report({ deleted: 0, failed: 0, aborted, uninstall_ok: false }));
    const cb = makeCb();
    await runBatchCleanup([app("A")], true, keyOf, cb);
    expect(cb.onResults.mock.calls.at(-1)?.[0]?.[0]?.status).toBe("failed");
    expect(cb.onDoneKeys).toHaveBeenCalledWith([]);
  });
  // F-1: `failed === 0` used to mark the app "ok" and count it as processed.
  // A selection the keep rules refused every row of is
  // `deleted: 0, failed: 0, skipped: N`, which is not a cleaned app.
  it("does not count an app as cleaned when the keep rules refused every row", async () => {
    const target = app("Refused");
    analyze.mockResolvedValue(scan("Refused", [item("C:\\Program Files\\Refused")]));
    fullCleanup.mockResolvedValue(report({ deleted: 0, failed: 0, skipped: 4, uninstall_ok: false }));
    const cb = makeCb();
    await runBatchCleanup([target], true, keyOf, cb, false, new Set([keyOf(target)]));
    expect(cb.onResults.mock.calls.at(-1)?.[0]?.[0]).toMatchObject({ status: "skipped" });
    expect(cb.onResults.mock.calls.at(-1)?.[0]?.[0]?.detail).toContain("refused 4");
    expect(cb.onDoneKeys).toHaveBeenCalledWith([]);
  });

  it("marks ok when leftovers cleaned", async () => {    analyze.mockResolvedValue(scan("A", [item("C:\\Program Files\\A")]));
    fullCleanup.mockResolvedValue(report({ deleted: 1, failed: 0 }));
    const cb = makeCb();
    await runBatchCleanup([app("A")], true, keyOf, cb);
    expect(cb.onDoneKeys).toHaveBeenCalledWith(["Uninstall\\A"]);
    expect(cb.onShowSummary).toHaveBeenCalledWith(true);
    expect(cb.busyRef.current).toBe(false);
  });

  it("defaults backup_enabled to false and honors opt-in true", async () => {
    analyze.mockResolvedValue(scan("A", [item("C:\\Program Files\\A")]));
    fullCleanup.mockResolvedValue(report({ deleted: 1, failed: 0 }));
    const cb = makeCb();
    await runBatchCleanup([app("A")], true, keyOf, cb);
    expect(fullCleanup).toHaveBeenCalledWith(
      expect.anything(),
      expect.anything(),
      expect.objectContaining({ backup_enabled: false, cleanup_source: "uninstall" }),
    );
    fullCleanup.mockClear();
    await runBatchCleanup([app("A")], true, keyOf, makeCb(), true);
    expect(fullCleanup).toHaveBeenCalledWith(
      expect.anything(),
      expect.anything(),
      expect.objectContaining({ backup_enabled: true }),
    );
  });

  it("stops the queue after a reboot-pending report", async () => {
    analyze.mockResolvedValue(scan("A", [item("C:\\Program Files\\A")]));
    fullCleanup.mockResolvedValue(
      report({
        deleted: 0,
        failed: 0,
        aborted: true,
        uninstall_ok: true,
        reboot_pending: true,
        uninstall_message: "system reboot pending",
      }),
    );
    const cb = makeCb();
    await runBatchCleanup([app("A"), app("B")], true, keyOf, cb);
    expect(analyze).toHaveBeenCalledTimes(1);
    expect(fullCleanup).toHaveBeenCalledTimes(1);
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("reboot");
    expect(last?.officialDone).toBe(true);
  });

  it("fails the row instead of scanning untracked when the task cap rejects begin", async () => {
    beginScan.mockRejectedValueOnce(new Error("scan:busy"));
    analyze.mockResolvedValue(scan("A", [item("C:\\Program Files\\A")]));
    const cb = makeCb();
    await runBatchCleanup([app("A")], true, keyOf, cb);
    expect(analyze).not.toHaveBeenCalled();
    expect(fullCleanup).not.toHaveBeenCalled();
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("failed");
  });

  it("maps cleanup abort to failed when items were present (S-R4-06)", async () => {
    analyze.mockResolvedValue(scan("A", [item("C:\\Program Files\\A")]));
    fullCleanup.mockResolvedValue(
      report({ deleted: 0, failed: 0, aborted: true, uninstall_message: "backup failed" }),
    );
    const cb = makeCb();
    await runBatchCleanup([app("A")], true, keyOf, cb);
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("failed");
    expect(String(last?.detail)).toContain("backup");
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

  it("does not mark a global pending gate as officially uninstalled", async () => {
    analyze.mockResolvedValue(scan("A", [item("C:\\Program Files\\A")]));
    fullCleanup.mockResolvedValue(
      report({
        deleted: 0,
        failed: 0,
        aborted: true,
        uninstall_ok: false,
        reboot_pending: true,
        uninstall_message: "system reboot pending; uninstall skipped",
      }),
    );
    const cb = makeCb();
    await runBatchCleanup([app("A")], true, keyOf, cb);
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("reboot");
    expect(last?.officialDone).toBe(false);
  });

  it("does not mark a cancelled in-flight analysis as a failed row", async () => {
    const cb = makeCb();
    analyze.mockImplementation(async () => {
      cb.cancelRef.current = true;
      throw new Error("scan:cancelled");
    });
    await runBatchCleanup([app("Cancelled")], true, keyOf, cb);
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("skipped");
    expect(fullCleanup).not.toHaveBeenCalled();
  });

  it("releases the reserved scan slot when analyze is refused before it starts", async () => {
    analyze.mockRejectedValueOnce(new Error("elevated HKCU analysis refused"));
    const cb = makeCb();
    await runBatchCleanup([app("PerUser")], true, keyOf, cb);
    expect(cancelScan).toHaveBeenCalledWith(7);
    expect(fullCleanup).not.toHaveBeenCalled();
    expect(cb.onResults.mock.calls.at(-1)?.[0]?.[0]?.status).toBe("failed");
  });

  it("keeps the current app's contract: cancel stops the queue, not the in-flight app", async () => {
    // The UI promises "stops after the current app finishes", so a cancel that
    // arrives while the current app is being analyzed must still let that app
    // complete and must not start the next one.
    analyze.mockImplementation(async () => {
      cb.cancelRef.current = true;
      return scan("Ordered", [item("C:\\Program Files\\Ordered")]);
    });
    fullCleanup.mockResolvedValue(report({ deleted: 1, failed: 0 }));
    const cb = makeCb();
    await runBatchCleanup([app("Ordered"), app("Second")], true, keyOf, cb);
    expect(analyze).toHaveBeenCalledTimes(1);
    expect(fullCleanup).toHaveBeenCalledTimes(1);
  });

  it("cancels the current scan through the task id it was given", async () => {
    vi.useFakeTimers();
    try {
      const cancelAssociationScan = vi.fn(async () => true);
      const apiMod = await import("../lib/api");
      (apiMod.api as unknown as { cancelAssociationScan: unknown }).cancelAssociationScan =
        cancelAssociationScan;
      let resolveAnalyze: (r: ScanResult) => void = () => {};
      analyze.mockImplementation(
        () => new Promise<ScanResult>((resolve) => { resolveAnalyze = resolve; }),
      );
      fullCleanup.mockResolvedValue(report({ deleted: 0, uninstall_ok: true }));
      const cb = makeCb();
      const run = runBatchCleanup([app("Slow")], true, keyOf, cb);
      cb.cancelRef.current = true;
      await vi.advanceTimersByTimeAsync(300);
      expect(cancelAssociationScan).toHaveBeenCalledWith(7);
      resolveAnalyze(scan("Slow", []));
      await run;
    } finally {
      vi.useRealTimers();
    }
  });

  it("maps invoke errors to failed rows", async () => {
    analyze.mockRejectedValue(new Error("boom"));
    const cb = makeCb();
    await runBatchCleanup([app("E")], true, keyOf, cb);
    const last = cb.onResults.mock.calls.at(-1)?.[0]?.[0];
    expect(last?.status).toBe("failed");
  });
});
