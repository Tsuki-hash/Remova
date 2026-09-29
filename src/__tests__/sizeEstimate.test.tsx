// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useSizeEstimate } from "../hooks/useSizeEstimate";
import type { InstalledApp } from "../types";

const native = vi.hoisted(() => ({ beginSizeEstimate: vi.fn(), cancelSizeEstimate: vi.fn(), estimateDirSizeKb: vi.fn() }));
vi.mock("../lib/api", () => ({ api: native }));
type Size = { kb: number; capped: boolean };
let complete: Array<(v: Size) => void>;
const apps = ["A", "B", "C"].map(name => ({ name, estimated_size_kb: 0, install_location: `C:\\${name}` }) as InstalledApp);
beforeEach(() => {
  vi.resetAllMocks(); complete = [];
  native.beginSizeEstimate.mockResolvedValue(undefined);
  native.cancelSizeEstimate.mockResolvedValue(undefined);
  native.estimateDirSizeKb.mockImplementation(() => new Promise<Size>(resolve => complete.push(resolve)));
});
afterEach(cleanup);
async function finishOutstanding() {
  await act(async () => complete.splice(0).forEach(resolve => resolve({ kb: 10, capped: false })));
}

it("stop remembers queued and in-flight paths when refresh beats responses", async () => {
  const { result, rerender } = renderHook(({ list }) => useSizeEstimate(list, false), { initialProps: { list: apps } });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2));
  await act(async () => result.current.stopSizeEstimate());
  rerender({ list: [...apps] });
  await finishOutstanding();
  expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2);
  expect(result.current.estimating).toBe(false);
  expect(result.current.sizeOf(apps[0]!)).toBe(0);
});

it("stop remembers queued paths after in-flight responses finish", async () => {
  const { result, rerender } = renderHook(({ list }) => useSizeEstimate(list, false), { initialProps: { list: apps } });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2));
  await act(async () => result.current.stopSizeEstimate());
  await finishOutstanding();
  rerender({ list: [...apps] });
  await act(async () => {});
  expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2);
});

it("ordinary refresh does not mark abandoned paths as explicitly stopped", async () => {
  const { result, rerender } = renderHook(({ list }) => useSizeEstimate(list, false), { initialProps: { list: apps.slice(0, 1) } });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(1));
  rerender({ list: apps.slice(0, 1) });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2));
  await finishOutstanding();
  await waitFor(() => expect(result.current.sizeOf(apps[0]!)).toBe(10));
  rerender({ list: apps.slice(0, 1) });
  expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2);
});

it("new begin waits for an earlier cancel before estimating a new path", async () => {
  let resolveCancel!: () => void;
  native.cancelSizeEstimate.mockImplementation(() => new Promise<void>(resolve => { resolveCancel = resolve; }));
  const { result, rerender } = renderHook(({ list }) => useSizeEstimate(list, false), { initialProps: { list: apps } });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2));
  let stopping!: Promise<void>;
  act(() => { stopping = result.current.stopSizeEstimate(); });
  await waitFor(() => expect(native.cancelSizeEstimate).toHaveBeenCalledTimes(1));
  const extra = { ...apps[0]!, install_location: "C:\\D" };
  rerender({ list: [...apps, extra] });
  await act(async () => {});
  expect(native.beginSizeEstimate).toHaveBeenCalledTimes(1);
  await act(async () => { resolveCancel(); await stopping; });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(3));
  expect(native.estimateDirSizeKb).toHaveBeenLastCalledWith("C:\\D");
  await finishOutstanding();
});
