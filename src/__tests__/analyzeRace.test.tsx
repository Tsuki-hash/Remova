// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { CleanupItem, InstalledApp, ScanResult } from "../types";

const { analyzeMock, suggestMock } = vi.hoisted(() => ({
  analyzeMock: vi.fn(),
  suggestMock: vi.fn(async () => []),
}));

vi.mock("../lib/api", () => ({
  api: { analyze: analyzeMock, suggestIgnoreRules: suggestMock,
    beginAssociationScan: vi.fn(async () => 1), cancelAssociationScan: vi.fn(async () => {}),
    associationScanProgress: vi.fn(async () => null) },
}));

import { useAnalyzeFlow } from "../hooks/useAnalyzeFlow";

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
      setResidualFromUninstall: () => {},
      setUninstallingKey: () => {},
      setUninstallStage: () => {},
      setEvidence: () => {},
    };
  });

  it("drops a stale scan that resolves after a newer one", async () => {
    let resolveA: (r: ScanResult) => void = () => {};
    let resolveB: (r: ScanResult) => void = () => {};
    analyzeMock
      .mockImplementationOnce(() => new Promise<ScanResult>((res) => (resolveA = res)))
      .mockImplementationOnce(() => new Promise<ScanResult>((res) => (resolveB = res)));

    const { result } = renderHook(() =>
      useAnalyzeFlow({
        goNav: () => {},
        flow: flow as never,
        refreshApps: async () => {},
        busyRef: { current: false },
      }),
    );

    const scanA = scan("AppA");
    const scanB = scan("AppB");

    await act(async () => {
      void result.current.analyze(app("AppA"));
    });
    await act(async () => {
      void result.current.analyze(app("AppB"));
    });
    expect(captured.scanning).toBe(true);

 // Newest finishes first.
    await act(async () => {
      resolveB(scanB);
    });
    expect(captured.scan).toBe(scanB);
 // The older request must not resurrect the spinner.
    expect(captured.scanning).toBe(false);

 // Stale response lands afterwards — it must be ignored entirely.
    await act(async () => {
      resolveA(scanA);
    });
    expect(captured.scan).toBe(scanB);
    expect(captured.selectedPaths).toEqual(new Set([scanB.items[0]?.path]));
    expect(captured.scanning).toBe(false);
  });

  it("clears the residual flag after a failed post-uninstall scan", async () => {
    const residual = vi.fn();
    flow.setResidualFromUninstall = residual;
    analyzeMock.mockRejectedValue(new Error("scan failed"));
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never, refreshApps: async () => {}, busyRef: { current: false } }));
    await act(async () => { await result.current.analyze(app("AppA"), { fromUninstall: true }); });
    expect(residual).toHaveBeenLastCalledWith(false);
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
    expect(captured.selectedPaths).toEqual(new Set([scan("AppB").items[0]?.path]));
    expect(captured.scanning).toBe(false);
  });
  it("clears old selection and never publishes a cancelled scan", async () => {
    let finish!: (scan: ScanResult) => void;
    analyzeMock.mockImplementationOnce(() => new Promise<ScanResult>(r => { finish = r; }));
    captured.selectedPaths.add("old-authorized-path");
    const { result } = renderHook(() => useAnalyzeFlow({ goNav: () => {}, flow: flow as never,
      refreshApps: async () => {}, busyRef: { current: false } }));
    let work!: Promise<void>;
    await act(async () => { work = result.current.analyze(app("AppA")); });
    expect(captured.selectedPaths.size).toBe(0);
    await act(async () => { await result.current.cancelScan(); finish(scan("AppA")); await work; });
    expect(captured.scan).toBeNull();
    expect(captured.selectedPaths.size).toBe(0);
    expect(captured.scanning).toBe(false);
    expect(result.current.scanProgress?.status).toBe("cancelled");
  });
});
