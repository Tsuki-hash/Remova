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
import { t } from "../i18n";

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
