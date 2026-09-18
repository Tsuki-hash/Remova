// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useAppCoreState } from "../hooks/useAppCoreState";
import { useResidualState } from "../hooks/useResidualState";

describe("FE-P0a functional setters", () => {
  it("setMulti applies updater instead of clearing", () => {
    const { result } = renderHook(() => useAppCoreState());
    act(() => result.current.setMulti(new Set(["a", "b", "c"])));
    expect(result.current.multi.size).toBe(3);
    act(() =>
      result.current.setMulti((m) => {
        const n = new Set(m);
        n.delete("a");
        return n;
      }),
    );
    expect(result.current.multi.has("a")).toBe(false);
    expect(result.current.multi.has("b")).toBe(true);
    expect(result.current.multi.has("c")).toBe(true);
  });

  it("setSelectedPaths functional update toggles without clearing all", () => {
    const { result } = renderHook(() => useResidualState());
    act(() => result.current.setSelectedPaths(new Set(["p1", "p2"])));
    act(() =>
      result.current.setSelectedPaths((s) => {
        const n = new Set(s);
        if (n.has("p1")) n.delete("p1");
        else n.add("p1");
        return n;
      }),
    );
    expect(result.current.selectedPaths.has("p1")).toBe(false);
    expect(result.current.selectedPaths.has("p2")).toBe(true);
  });
});
