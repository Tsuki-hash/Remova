// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { CleanupItem, InstalledApp, ScanResult } from "../types";

const { analyzeMock, suggestMock } = vi.hoisted(() => ({
  analyzeMock: vi.fn(),
  suggestMock: vi.fn(async () => []),
}));

vi.mock("../lib/api", () => ({
  api: { analyze: analyzeMock, suggestIgnoreRules: suggestMock },
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

    act(() => {
      void result.current.analyze(app("AppA"));
    });
    act(() => {
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

  it("a settled scan can be re-run and replaces the previous result", async () => {
    let calls = 0;
    analyzeMock.mockImplementation(() => {
      calls += 1;
      return Promise.resolve(scan("AppA"));
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
  });
});
