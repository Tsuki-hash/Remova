// @vitest-environment jsdom
import { cleanupItemKey } from "../lib/cleanupPlan";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { CleanupItem, InstalledApp, ScanResult } from "../types";

const { analyzeMock, suggestMock, officialMock } = vi.hoisted(() => ({
  analyzeMock: vi.fn(),
  suggestMock: vi.fn(async () => []),
  officialMock: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: { analyze: analyzeMock, suggestIgnoreRules: suggestMock, officialUninstall: officialMock,
    beginAssociationScan: vi.fn(async () => 1), cancelAssociationScan: vi.fn(async () => {}),
    associationScanProgress: vi.fn(async () => null) },
}));
vi.mock("../lib/confirm", () => ({
  requestConfirmEx: vi.fn(async () => ({ ok: true, checked: true })),
  requestConfirm: vi.fn(async () => true),
}));

import { useAnalyzeFlow } from "../hooks/useAnalyzeFlow";
import { getToasts, toast } from "../lib/toast";
import { t } from "../i18n";

afterEach(() => toast.clear());

function app(name: string): InstalledApp {
  return {
    name,
    version: "1.0",
    publisher: "Acme",
    install_location: `C:\\Program Files\\${name}`,
    uninstall_string: "",
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: "",
    estimated_size_kb: 0,
    install_date: "",
    display_icon: "",
  };
}

function selectableItem(path: string): CleanupItem {
  return {
    path,
    kind: "dir",
    score: 90,
    confidence: "confirmed",
    risk: "low",
    reason: "install dir",
    evidence: [],
    shared: false,
    user_data: false,
  };
}

function scan(name: string): ScanResult {
  return { app_name: name, items: [selectableItem(`C:\\Program Files\\${name}`)] };
}

type Captured = {
  scan: ScanResult | null;
  scanning: boolean;
  selectedPaths: Set<string>;
  selected: InstalledApp | null;
};

describe("useAnalyzeFlow request sequencing", () => {
  let captured: Captured;
  let flow: Record<string, unknown>;

  beforeEach(() => {
    analyzeMock.mockReset();
    officialMock.mockReset();
    suggestMock.mockClear();
    captured = { scan: null, scanning: false, selectedPaths: new Set(), selected: null };
    flow = {
      setSelected: (a: InstalledApp | null) => {
        captured.selected = a;
      },
      setScanning: (v: boolean) => {
        captured.scanning = v;
      },
      setScan: (r: ScanResult | null) => {
        captured.scan = r;
      },
      setReport: () => {},
      setAiNotes: () => {},
      setAiRisk: () => {},
      setIgnoreSuggestions: () => {},
      setSelectedPaths: (s: Set<string>) => {
        captured.selectedPaths = s;
      },
      setError: () => {},
      setUninstallReview: () => {},
      updateUninstallRescan: () => {},
      setUninstallingKey: () => {},
      setUninstallStage: () => {},
      setEvidence: () => {},
    };
  });

  it("rejects a new analysis while another scan owns the foreground", async () => {
    let resolveA!: (scan: ScanResult) => void;
    analyzeMock.mockImplementationOnce(() => new Promise<ScanResult>(resolve => { resolveA = resolve; }));
    const busyRef = { current: false };
    const { result } = renderHook(() => useAnalyzeFlow({
      goNav: () => {}, flow: flow as never, refreshApps: async () => {}, busyRef,
    }));
    let work!: Promise<boolean>;
    await act(async () => { work = result.current.analyze(app("AppA")); });
    await act(async () => { expect(await result.current.analyze(app("AppB"))).toBe(false); });
    expect(analyzeMock).toHaveBeenCalledTimes(1);
    expect(captured.selected?.name).toBe("AppA");
    expect(captured.scanning).toBe(true);
    expect(busyRef.current).toBe(true);
    await act(async () => { resolveA(scan("AppA")); await work; });
    expect(captured.scan?.app_name).toBe("AppA");
    expect(captured.scanning).toBe(false);
    expect(busyRef.current).toBe(false);
  });

  it.each([
    ["scan failed", "failed"], ["scan:cancelled", "cancelled"],
    ["uninstall review expired", "expired"], ["uninstall review found an installed application", "installed"],
    ["uninstall review could not verify installation", "unverified"],
  ] as const)("keeps the uninstall review when the rescan returns %s", async (error, phase) => {
    const residual = vi.fn();
    flow.updateUninstallRescan = residual;
    analyzeMock.mockRejectedValue(new Error(error));
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never, refreshApps: async () => {}, busyRef: { current: false } }));
    await act(async () => { await result.current.analyze(app("AppA"), { fromUninstall: true, uninstallReview: 42 }); });
    expect(residual).toHaveBeenLastCalledWith({ app: app("AppA"), receipt: 42, rescan: { phase } });
    expect(captured.scan).toBeNull();
    expect(captured.scanning).toBe(false);
  });

  it("a settled scan can be re-run and replaces the previous result", async () => {
    let calls = 0;
    analyzeMock.mockImplementation(() => {
      calls += 1;
      return Promise.resolve(scan(calls === 1 ? "AppA" : "AppB"));
    });
    const { result } = renderHook(() =>
      useAnalyzeFlow({
        goNav: () => {},
        flow: flow as never,
        refreshApps: async () => {},
        busyRef: { current: false },
      }),
    );

    await act(async () => {
      await result.current.analyze(app("AppA"));
    });
    expect(calls).toBe(1);
    expect(captured.scan?.app_name).toBe("AppA");

 // a second run must replace the previous result.
    await act(async () => {
      await result.current.analyze(app("AppB"));
    });
    expect(calls).toBe(2);
    expect(captured.scan?.app_name).toBe("AppB");
    expect(captured.selectedPaths).toEqual(new Set([cleanupItemKey(scan("AppB").items[0]!)]));
    expect(captured.scanning).toBe(false);
  });
  it("clears old selection and never publishes a cancelled scan", async () => {
    let finish!: (scan: ScanResult) => void;
    analyzeMock.mockImplementationOnce(() => new Promise<ScanResult>(r => { finish = r; }));
    captured.selectedPaths.add("old-authorized-path");
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef: { current: false } }));
    let work!: Promise<boolean>;
    await act(async () => { work = result.current.analyze(app("AppA")); });
    expect(captured.selectedPaths.size).toBe(0);
    await act(async () => { await result.current.cancelScan(); finish(scan("AppA")); await work; });
    expect(captured.scan).toBeNull();
    expect(captured.selectedPaths.size).toBe(0);
    expect(captured.scanning).toBe(false);
    expect(result.current.scanProgress?.status).toBe("cancelled");
  });

  // `forceClean` and `startUninstall` guard on `busyRef.current` only, and
  // `analyze` never set it. A destructive action could therefore start while a
  // deep scan was in flight, and the scan then landed on top of that action's
  // report (it never increments `analyzeSeqRef`).
  it("holds busyRef for the whole scan and releases it afterwards", async () => {
    let finish!: (scan: ScanResult) => void;
    analyzeMock.mockImplementationOnce(() => new Promise<ScanResult>(r => { finish = r; }));
    const busyRef = { current: false };
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef }));
    let work!: Promise<boolean>;
    await act(async () => { work = result.current.analyze(app("AppA")); });
    expect(busyRef.current, "a destructive action must be blocked mid-scan").toBe(true);
    await act(async () => { finish(scan("AppA")); await work; });
    expect(busyRef.current, "the guard must not leak once the scan settles").toBe(false);
  });

  it("rejects externally requested review scans while another task owns the foreground", async () => {
    const busyRef = { current: true };
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef }));
    await act(async () => {
      expect(await result.current.analyze(app("AppA"), { fromUninstall: true, uninstallReview: 42, afterCleanup: true })).toBe(false);
    });
    expect(analyzeMock).not.toHaveBeenCalled();
    expect(captured.selected).toBeNull();
    expect(busyRef.current).toBe(true);
  });

  it("skips the post-uninstall rescan when the uninstaller initiated a reboot", async () => {
    officialMock.mockResolvedValue({
      ok: true, had_command: true, reboot_initiated: true,
      message: "uninstaller finished (reboot required): msiexec.exe",
    });
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef: { current: false } }));
    await act(async () => { await result.current.startUninstall(app("RebootApp")); });
    expect(analyzeMock).not.toHaveBeenCalled();
  });

  it("official-only uninstall skips the rescan after a reboot initiation", async () => {
    officialMock.mockResolvedValue({
      ok: true, had_command: true, reboot_initiated: true,
      message: "uninstaller finished (reboot required): msiexec.exe",
    });
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef: { current: false } }));
    await act(async () => { await result.current.openOfficialOnly(app("OfficialRebootApp")); });
    expect(officialMock).toHaveBeenCalled();
    expect(analyzeMock).not.toHaveBeenCalled();
  });

  it.each(["startUninstall", "openOfficialOnly"] as const)("shows restart guidance and holds residual cleanup in %s", async action => {
    officialMock.mockResolvedValue({
      ok: false, had_command: true, reboot_initiated: false,
      message: "uninstall requires reboot before verification",
    });
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef: { current: false } }));
    await act(async () => { await result.current[action](app("NeedsRestart")); });
    expect(getToasts()).toContainEqual(expect.objectContaining({ kind: "info", message: t().backendUninstallRebootBeforeVerify }));
    expect(getToasts().some(item => item.message === t().uninstallOk)).toBe(false);
    expect(analyzeMock).not.toHaveBeenCalled();
  });

  it("clears the previous scan and path selection when another app is selected", () => {
    captured.scan = scan("Old");
    captured.selectedPaths = new Set([scan("Old").items[0]?.path ?? "C:\\Old"]);
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef: { current: false } }));
    act(() => result.current.selectApp(app("New")));
    expect(captured.selected?.name).toBe("New");
    expect(captured.scan).toBeNull();
    expect(captured.selectedPaths.size).toBe(0);
  });

  it("rescans after a normal official uninstall when the preference is on", async () => {
    officialMock.mockResolvedValue({
      ok: true, had_command: true, reboot_initiated: false, uninstall_review: 42,
      message: "uninstaller finished: setup.exe",
    });
    analyzeMock.mockResolvedValue(scan("NormalApp"));
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef: { current: false } }));
    await act(async () => { await result.current.startUninstall(app("NormalApp")); });
    // The scan itself is the observable effect of analyze({fromUninstall}).
    expect(analyzeMock).toHaveBeenCalledWith(
      expect.objectContaining({ name: "NormalApp" }),
      expect.anything(),
      42,
    );
  });
});
