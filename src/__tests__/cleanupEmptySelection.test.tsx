// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { ScanResult } from "../types";
import type { CleanupFlowSetters } from "../hooks/useCleanupHandlers";

const requestConfirmEx = vi.fn();
const fullCleanup = vi.fn();

vi.mock("../lib/confirm", () => ({
  requestConfirmEx: (...a: unknown[]) => requestConfirmEx(...a),
  requestConfirm: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: {
    fullCleanup: (...a: unknown[]) => fullCleanup(...a),
    analyze: vi.fn(),
    aiRiskBrief: vi.fn(),
    verifyLeftovers: vi.fn(),
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

function baseScan(): ScanResult {
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

function noop() {}

const flow: CleanupFlowSetters = {
  setMulti: noop,
  setResidualFromUninstall: noop,
  setAiRisk: noop,
  setReport: noop,
  setLastReport: noop,
  setVerifyRows: noop,
  setAiReportNote: noop,
  setError: noop,
};

describe("useCleanupHandlers empty selection (FE-N3 / T-R4-07)", () => {
  beforeEach(() => {
    requestConfirmEx.mockReset();
    fullCleanup.mockReset();
  });

  it("does not open confirm or cleanup when selectedPaths is empty", async () => {
    const { result } = renderHook(() =>
      useCleanupHandlers({
        L: t(),
        selected: null,
        scan: baseScan(),
        selectedPaths: new Set<string>(),
        residualFromUninstall: false,
        useOfficial: false,
        aiEnabled: false,
        aiRisk: null,
        apps: [],
        multi: new Set<string>(),
        flow,
        refreshApps: async () => {},
        busyRef: { current: false },
      }),
    );
    await act(async () => {
      await result.current.handleCleanupConfirm();
    });
    expect(requestConfirmEx).not.toHaveBeenCalled();
    expect(fullCleanup).not.toHaveBeenCalled();
  });
});
