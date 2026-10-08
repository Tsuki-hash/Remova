// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { ManageItem } from "../types";
import type { ManageTabId } from "../lib/useManageList";
const native = vi.hoisted(() => ({ listStartupItems: vi.fn(), listServices: vi.fn(), listScheduledTasks: vi.fn() }));
vi.mock("../lib/api", () => ({ api: native }));
import { useManageList } from "../lib/useManageList";
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it.each(["success", "failure"])("ignores an old management %s after switching tabs", async outcome => {
  let finish!: (items: ManageItem[]) => void;
  let fail!: (error: Error) => void;
  native.listStartupItems.mockImplementationOnce(() => new Promise<ManageItem[]>((resolve, reject) => { finish = resolve; fail = reject; }));
  native.listServices.mockResolvedValueOnce([{ name: "current-service", location: "service", detail: "", enabled: true }]);
  const onError = vi.fn();
  const view = renderHook(({ tab }: { tab: ManageTabId }) => useManageList(tab, onError), { initialProps: { tab: "startup" as ManageTabId } });
  view.rerender({ tab: "services" });
  await waitFor(() => expect(view.result.current.items[0]?.name).toBe("current-service"));
  await act(async () => { if (outcome === "success") finish([{ name: "old-startup", location: "startup", detail: "", enabled: true }]); else fail(new Error("late failure")); });
  expect(view.result.current.items[0]?.name).toBe("current-service");
  expect(view.result.current.busy).toBe(false);
  expect(onError).not.toHaveBeenCalled();
});

it("ignores failures after exit and prevents overlapping refreshes", async () => {
  let fail!: (error: Error) => void;
  native.listScheduledTasks.mockImplementationOnce(() => new Promise<ManageItem[]>((_resolve, reject) => { fail = reject; }));
  const onError = vi.fn();
  const view = renderHook(() => useManageList("tasks", onError));
  await act(async () => { await view.result.current.reload(); });
  expect(native.listScheduledTasks).toHaveBeenCalledOnce();
  view.unmount();
  await act(async () => { fail(new Error("late failure")); });
  expect(onError).not.toHaveBeenCalled();
});
