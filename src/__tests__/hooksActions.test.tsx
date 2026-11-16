// @vitest-environment jsdom
import { cleanupItemKey } from "../lib/cleanupPlan";
import { describe, expect, it } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useResidualState } from "../hooks/useResidualState";
import { useAiPanelState } from "../hooks/useAiPanelState";
import type { CleanupItem } from "../types";

function item(partial: Partial<CleanupItem> = {}): CleanupItem {
  return {
    path: "C:\\a",
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

describe("useResidualState actions", () => {
  it("togglePath / selectDefaultItems / clearResidualScan", () => {
    const { result } = renderHook(() => useResidualState());
    act(() => result.current.actions.togglePath(cleanupItemKey(item())));
    expect(result.current.selectedPaths.has(cleanupItemKey(item()))).toBe(true);
    act(() => result.current.actions.selectDefaultItems([
      item(),
      item({ path: "C:\\shared", shared: true }),
    ]));
    expect(result.current.selectedPaths.has(cleanupItemKey(item()))).toBe(true);
    expect(result.current.selectedPaths.has(cleanupItemKey(item({ path: "C:\\shared" })))).toBe(false);
    act(() => result.current.actions.clearResidualScan());
    expect(result.current.selectedPaths.size).toBe(0);
  });
});

describe("useAiPanelState actions", () => {
  it("clearAiScanState and copilot filter", () => {
    const { result } = renderHook(() => useAiPanelState());
    act(() => {
      result.current.setAiNotes({ "C:\\a": "note" });
      result.current.setAiRisk("high");
    });
    act(() => result.current.actions.clearAiScanState());
    expect(result.current.aiNotes).toEqual({});
    expect(result.current.aiRisk).toBeNull();
  });
});
