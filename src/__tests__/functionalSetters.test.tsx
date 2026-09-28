// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useAppCoreState } from "../hooks/useAppCoreState";
import { useResidualState } from "../hooks/useResidualState";
import { useShellState } from "../hooks/useShellState";

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

  it("setMulti consecutive functional updates do not drop (FE-N1)", () => {
    const { result } = renderHook(() => useAppCoreState());
    act(() => result.current.setMulti(new Set(["a", "b"])));
    act(() =>
      result.current.setMulti((m) => {
        const n = new Set(m);
        n.delete("a");
        return n;
      }),
    );
    act(() =>
      result.current.setMulti((m) => {
        const n = new Set(m);
        n.add("z");
        return n;
      }),
    );
    expect(result.current.multi.has("a")).toBe(false);
    expect(result.current.multi.has("b")).toBe(true);
    expect(result.current.multi.has("z")).toBe(true);
    expect(result.current.multi.size).toBe(2);
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

  it("shell setTheme/setLangVer/setShowDetail apply real updaters (C-01)", () => {
    const { result } = renderHook(() => useShellState());
    const startTheme = result.current.theme;
    const flipped = startTheme === "dark" ? "light" : "dark";
    act(() => result.current.setTheme((t) => (t === "dark" ? "light" : "dark")));
    expect(result.current.theme).toBe(flipped);
    act(() => result.current.setTheme(flipped));
    expect(result.current.theme).toBe(flipped);

    act(() => result.current.setLangVer((v) => v + 5));
    expect(result.current.langVer).toBe(5);
    act(() => result.current.setLangVer(2));
    expect(result.current.langVer).toBe(2);

    act(() => result.current.setShowDetail((s) => !s));
    expect(result.current.showDetail).toBe(false);
    act(() => result.current.setShowDetail(true));
    expect(result.current.showDetail).toBe(true);
  });
});
