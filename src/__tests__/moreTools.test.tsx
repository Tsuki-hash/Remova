// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { useMoreTools } from "../hooks/useMoreTools";
import type { InstalledApp } from "../types";
import { toast } from "../lib/toast";

vi.mock("../lib/toast", () => ({ toast: { info: vi.fn() } }));
afterEach(cleanup);
beforeEach(() => { localStorage.clear(); vi.clearAllMocks(); });

it("requires a selected app before running a tool and routes the user to software", () => {
  const run = vi.fn(), onGoSoftware = vi.fn();
  const { result, rerender } = renderHook(({ selected }) => useMoreTools({ selected, onGoSoftware }), {
    initialProps: { selected: null as InstalledApp | null },
  });
  act(() => result.current.requireSelection(run));
  expect(run).not.toHaveBeenCalled();
  expect(onGoSoftware).toHaveBeenCalledOnce();
  expect(toast.info).toHaveBeenCalledOnce();
  rerender({ selected: { name: "Demo" } as InstalledApp });
  act(() => result.current.requireSelection(run));
  expect(run).toHaveBeenCalledOnce();
  expect(onGoSoftware).toHaveBeenCalledOnce();
});

it("toggles panels and persists the rescan choice across mounts", () => {
  const opts = { selected: null, onGoSoftware: vi.fn() };
  const first = renderHook(() => useMoreTools(opts));
  expect(first.result.current.rescanOn).toBe(true);
  act(() => first.result.current.toggleTool("history"));
  expect(first.result.current.openTool).toBe("history");
  act(() => first.result.current.toggleTool("restore"));
  expect(first.result.current.openTool).toBe("restore");
  act(() => first.result.current.toggleTool("restore"));
  expect(first.result.current.openTool).toBeNull();
  act(() => first.result.current.toggleRescan());
  expect(first.result.current.rescanOn).toBe(false);
  first.unmount();
  expect(renderHook(() => useMoreTools(opts)).result.current.rescanOn).toBe(false);
});
