// @vitest-environment jsdom
// REV-QA-02: money-path orchestration — confirm pipeline + busyRef gate.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { FullCleanupReport, InstalledApp, ScanResult } from "../types";
import type { CleanupFlowSetters } from "../hooks/useCleanupHandlers";

const requestConfirmEx = vi.fn();
const fullCleanup = vi.fn();
const analyze = vi.fn();

vi.mock("../lib/confirm", () => ({
  requestConfirmEx: (...a: unknown[]) => requestConfirmEx(...a),
  requestConfirm: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: {
    fullCleanup: (...a: unknown[]) => fullCleanup(...a),
    analyze: (...a: unknown[]) => analyze(...a),
    aiRiskBrief: vi.fn(),
    verifyLeftovers: vi.fn(),
    dryRun: vi.fn(),
  },
}));

vi.mock("../lib/toast", () => ({
  toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() },
}));

vi.mock("../lib/batchEngine", () => ({
  runBatchCleanup: vi.fn(),
}));

import { useCleanupHandlers } from "../hooks/useCleanupHandlers";
import { runBatchCleanup } from "../lib/batchEngine";
import { toast } from "../lib/toast";

const runBatchCleanupMock = vi.mocked(runBatchCleanup);
import { t } from "../i18n";
import { appKey } from "../lib/appKey";

function app(): InstalledApp {
  return {
    name: "DemoApp",
    version: "1",
    publisher: "P",
    install_location: "C:\\Program Files\\DemoApp",
    uninstall_string: "",
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: "k",
    estimated_size_kb: 0,
    install_date: "",
    display_icon: "",
  };
}

function scan(): ScanResult {
  return {
    app_name: "DemoApp",
    items: [
      {
        path: "C:\\Program Files\\DemoApp\\x",
        kind: "dir",
        score: 90,
        confidence: "confirmed",
        risk: "low",
        reason: "install",
        evidence: [],
        shared: false,
        user_data: false,
      },
    ],
  };
}

function report(partial: Partial<FullCleanupReport> = {}): FullCleanupReport {
  return {
    app_name: "DemoApp",
    dry_run: false,
    backup_dir: "",
    uninstall_ok: true,
    uninstall_message: "",
    deleted: 1,
    failed: 0,
    skipped: 0,
    delayed: 0,
    aborted: false,
    restore_point_ok: false,
    restore_point_msg: "",
    errors: [],
    details: [],
    ...partial,
  } as FullCleanupReport;
}

const noop = () => {};
const flow: CleanupFlowSetters = {
  setMulti: noop,
  setResidualFromUninstall: noop,
  setAiRisk: noop,
  setReport: noop,
  setVerifyRows: noop,
  setAiReportNote: noop,
  setError: noop,
};

function setup(selectedPaths = new Set(["C:\\Program Files\\DemoApp\\x"])) {
  const busyRef = { current: false };
  const hook = renderHook(() =>
    useCleanupHandlers({
      L: t(),
      selected: app(),
      scan: scan(),
      selectedPaths,
      residualFromUninstall: false,
      useOfficial: false,
      aiEnabled: false,
      aiRisk: null,
      apps: [],
      multi: new Set<string>(),
      flow,
      refreshApps: async () => {},
      busyRef,
    }),
  );
  return { ...hook, busyRef };
}

describe("useCleanupHandlers confirm / force pipeline (REV-QA-02)", () => {
  beforeEach(() => {
    requestConfirmEx.mockReset();
    fullCleanup.mockReset();
    analyze.mockReset();
  });

  it("forceClean cancel does not call fullCleanup and frees busyRef", async () => {
    analyze.mockResolvedValue(scan());
    requestConfirmEx.mockResolvedValue({ ok: false, checked: false });
    const { result, busyRef } = setup();
    await act(async () => {
      await result.current.forceClean();
    });
    expect(requestConfirmEx).toHaveBeenCalled();
    expect(fullCleanup).not.toHaveBeenCalled();
    expect(busyRef.current).toBe(false);
  });

  it("forceClean confirm runs fullCleanup with backup flag", async () => {
    analyze.mockResolvedValue(scan());
    requestConfirmEx.mockResolvedValue({ ok: true, checked: true });
    fullCleanup.mockResolvedValue(report());
    const { result, busyRef } = setup();
    await act(async () => {
      await result.current.forceClean();
    });
    expect(fullCleanup).toHaveBeenCalledTimes(1);
    const opts = fullCleanup.mock.calls[0]![2] as { backup_enabled: boolean; skip_official_uninstall: boolean };
    expect(opts.backup_enabled).toBe(true);
    expect(opts.skip_official_uninstall).toBe(true);
    expect(busyRef.current).toBe(false);
  });

  it("forceClean reentry blocked while busyRef is held", async () => {
    let release!: (v: unknown) => void;
    analyze.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    requestConfirmEx.mockResolvedValue({ ok: false, checked: false });
    const { result, busyRef } = setup();
    let p!: Promise<void>;
    act(() => {
      p = result.current.forceClean();
    });
    expect(busyRef.current).toBe(true);
    await act(async () => {
      await result.current.forceClean();
    });
    // Second call no-ops while first is in flight
    expect(analyze).toHaveBeenCalledTimes(1);
    await act(async () => {
      release(scan());
      await p;
    });
    expect(busyRef.current).toBe(false);
  });

  it("dryRun refuses to start when busyRef already held", async () => {
    const { result, busyRef } = setup();
    busyRef.current = true;
    await act(async () => {
      await result.current.dryRun();
    });
    // Guarded entry: stays true and never reaches api.dryRun (not called in mock).
    expect(busyRef.current).toBe(true);
    expect(fullCleanup).not.toHaveBeenCalled();
    expect(analyze).not.toHaveBeenCalled();
  });
});

describe("useCleanupHandlers deep pipeline (REV-QA-02/03)", () => {
  const mkApp = (n: string): InstalledApp => ({ ...app(), name: n, registry_key: n });

  function setupDeep(
    opts: {
      selectedPaths?: Set<string>;
      apps?: InstalledApp[];
      multi?: Set<string>;
    } = {},
  ) {
    const busyRef = { current: false };
    const refreshApps = vi.fn().mockResolvedValue(undefined);
    const flowDeep: CleanupFlowSetters = {
      setMulti: vi.fn(),
      setResidualFromUninstall: noop,
      setAiRisk: noop,
      setReport: vi.fn(),
      setVerifyRows: noop,
      setAiReportNote: noop,
      setError: vi.fn(),
    };
    const hook = renderHook(() =>
      useCleanupHandlers({
        L: t(),
        selected: app(),
        scan: scan(),
        selectedPaths: opts.selectedPaths ?? new Set(["C:\\Program Files\\DemoApp\\x"]),
        residualFromUninstall: false,
        useOfficial: false,
        aiEnabled: false,
        aiRisk: null,
        apps: opts.apps ?? [],
        multi: opts.multi ?? new Set<string>(),
        flow: flowDeep,
        refreshApps,
        busyRef,
      }),
    );
    return { ...hook, busyRef, refreshApps, flowDeep };
  }

  beforeEach(() => {
    requestConfirmEx.mockReset();
    fullCleanup.mockReset();
    analyze.mockReset();
    runBatchCleanupMock.mockReset();
    vi.mocked(toast.info).mockClear();
    vi.mocked(toast.error).mockClear();
    vi.mocked(toast.success).mockClear();
  });

  it("handleCleanupConfirm cancel frees busy without executing", async () => {
    requestConfirmEx.mockResolvedValue({ ok: false, checked: false });
    const { result, busyRef } = setupDeep();
    await act(async () => {
      await result.current.handleCleanupConfirm();
    });
    expect(requestConfirmEx).toHaveBeenCalledTimes(1);
    expect(fullCleanup).not.toHaveBeenCalled();
    expect(busyRef.current).toBe(false);
  });

  it("handleCleanupConfirm confirm executes with the checkbox backup flag", async () => {
    requestConfirmEx.mockResolvedValue({ ok: true, checked: true });
    fullCleanup.mockResolvedValue(report());
    const { result, busyRef, flowDeep } = setupDeep();
    await act(async () => {
      await result.current.handleCleanupConfirm();
    });
    expect(fullCleanup).toHaveBeenCalledTimes(1);
    const opts = fullCleanup.mock.calls[0]![2] as { backup_enabled: boolean };
    expect(opts.backup_enabled).toBe(true);
    expect(flowDeep.setReport).toHaveBeenCalled();
    expect(busyRef.current).toBe(false);
  });

  it("handleCleanupConfirm with empty selection only hints", async () => {
    const { result, busyRef } = setupDeep({ selectedPaths: new Set() });
    await act(async () => {
      await result.current.handleCleanupConfirm();
    });
    expect(requestConfirmEx).not.toHaveBeenCalled();
    expect(toast.info).toHaveBeenCalledWith(t().selectRowHint);
    expect(busyRef.current).toBe(false);
  });

  it("batchCleanup with no selected apps only hints", async () => {
    const { result } = setupDeep({ apps: [], multi: new Set() });
    await act(async () => {
      await result.current.batchCleanup();
    });
    expect(requestConfirmEx).not.toHaveBeenCalled();
    expect(runBatchCleanupMock).not.toHaveBeenCalled();
    expect(toast.info).toHaveBeenCalledWith(t().selectRowHint);
  });

  it("batchCleanup confirm runs the engine and refreshes the list", async () => {
    const a1 = mkApp("AppA");
    const a2 = mkApp("AppB");
    runBatchCleanupMock.mockImplementation(async (_q, _u, _k, cb) => {
      // Mirror the engine contract: it owns the busy slot for the whole run
      // and reports finished keys so multi can be pruned.
      cb.busyRef.current = true;
      cb.onDoneKeys([appKey(a1)]);
      cb.busyRef.current = false;
    });
    requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    const { result, busyRef, refreshApps, flowDeep } = setupDeep({
      apps: [a1, a2],
      multi: new Set([appKey(a1), appKey(a2)]),
    });
    await act(async () => {
      await result.current.batchCleanup();
    });
    expect(runBatchCleanupMock).toHaveBeenCalledTimes(1);
    const [queue, useOfficial, , , backup] = runBatchCleanupMock.mock.calls[0]! as [
      InstalledApp[],
      boolean,
      unknown,
      unknown,
      boolean,
    ];
    expect(queue).toHaveLength(2);
    expect(useOfficial).toBe(true); // beginner batch runs official uninstallers
    expect(backup).toBe(false);
    expect(refreshApps).toHaveBeenCalledTimes(1);
    expect(busyRef.current).toBe(false);
    expect(flowDeep.setMulti).toHaveBeenCalled(); // onDoneKeys pruning
  });

  it("batchCleanup cancel never reaches the engine", async () => {
    const a1 = mkApp("AppA");
    requestConfirmEx.mockResolvedValue({ ok: false, checked: false });
    const { result } = setupDeep({ apps: [a1], multi: new Set([appKey(a1)]) });
    await act(async () => {
      await result.current.batchCleanup();
    });
    expect(requestConfirmEx).toHaveBeenCalledTimes(1);
    expect(runBatchCleanupMock).not.toHaveBeenCalled();
  });

  it("REV-FE-10: busyRef taken while the dialog was open blocks the engine", async () => {
    const a1 = mkApp("AppA");
    const hook = setupDeep({ apps: [a1], multi: new Set([appKey(a1)]) });
    const busyRefRef = hook.busyRef;
    requestConfirmEx.mockImplementation(async () => {
      // Another cleanup wins the busy slot while the dialog is open.
      busyRefRef.current = true;
      return { ok: true, checked: false };
    });
    await act(async () => {
      await hook.result.current.batchCleanup();
    });
    expect(runBatchCleanupMock).not.toHaveBeenCalled();
    // The other owner still holds the slot — batchCleanup must not free it.
    expect(hook.busyRef.current).toBe(true);
  });

  it("retryFailedBatch re-arms failed keys and hides the summary", async () => {
    const a1 = mkApp("AppA");
    const a2 = mkApp("AppB");
    runBatchCleanupMock.mockImplementation(async (_q, _u, _k, cb) => {
      cb.onSetBatching(true);
      cb.onResults([
        { key: appKey(a1), name: a1.name, status: "failed", detail: "x" },
        { key: appKey(a2), name: a2.name, status: "ok", detail: "" },
      ]);
      cb.onShowSummary(true);
    });
    requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    const { result, flowDeep } = setupDeep({
      apps: [a1, a2],
      multi: new Set([appKey(a1), appKey(a2)]),
    });
    await act(async () => {
      await result.current.batchCleanup();
    });
    expect(result.current.showBatchSummary).toBe(true);
    act(() => {
      result.current.retryFailedBatch();
    });
    expect(flowDeep.setMulti).toHaveBeenCalledWith(new Set([appKey(a1)]));
    expect(result.current.showBatchSummary).toBe(false);
  });

  it("cancelBatch signals the cancel ref and toasts", () => {
    const { result } = setupDeep();
    act(() => {
      result.current.cancelBatch();
    });
    expect(toast.info).toHaveBeenCalledWith(t().batchCancelHint);
  });
});
