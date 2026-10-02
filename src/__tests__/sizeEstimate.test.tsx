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

it("changed app version at the same path invalidates a completed estimate", async () => {
  native.estimateDirSizeKb.mockResolvedValue({ kb: 10, capped: false });
  const list = [{ ...apps[0]!, version: "1" }];
  const { result, rerender } = renderHook(({ list }) => useSizeEstimate(list, false), { initialProps: { list } });
  await waitFor(() => expect(result.current.sizeOf(list[0]!)).toBe(10));
  native.estimateDirSizeKb.mockResolvedValue({ kb: 20, capped: false });
  const newer = [{ ...list[0]!, version: "2" }];
  rerender({ list: newer });
  await waitFor(() => expect(result.current.sizeOf(newer[0]!)).toBe(20));
  expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2);
});

it("explicit reload clears stopped paths and discards older in-flight responses", async () => {
  const list = apps.slice(0, 1);
  const { result, rerender } = renderHook(({ loading }) => useSizeEstimate(list, loading), { initialProps: { loading: false } });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(1));
  const old = complete.shift()!;
  await act(async () => result.current.stopSizeEstimate());
  rerender({ loading: true });
  rerender({ loading: false });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2));
  await act(async () => old({ kb: 100, capped: false }));
  expect(result.current.sizeOf(list[0]!)).toBe(0);
  await finishOutstanding();
  await waitFor(() => expect(result.current.sizeOf(list[0]!)).toBe(10));
});
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

it("a zero-KB success is not frozen into the cache — next refresh retries", async () => {
  native.estimateDirSizeKb.mockImplementation((path: string) =>
    path === "C:\\B" ? Promise.resolve({ kb: 0, capped: false }) : Promise.resolve({ kb: 10, capped: false }),
  );
  const { result, rerender } = renderHook(({ list }) => useSizeEstimate(list, false), { initialProps: { list: apps } });
  await waitFor(() => expect(result.current.estimating).toBe(false));
  expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(3);
  expect(result.current.sizeOf(apps[1]!)).toBe(0);
  rerender({ list: [...apps] });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(4));
  expect(native.estimateDirSizeKb).toHaveBeenLastCalledWith("C:\\B");
});

it("update:busy failures are neither cached nor skipped — next refresh retries", async () => {
  let busyFired = false;
  native.estimateDirSizeKb.mockImplementation((path: string) => {
    if (path === "C:\\A" && !busyFired) {
      busyFired = true;
      return Promise.reject(new Error("update:busy"));
    }
    return new Promise<Size>(resolve => complete.push(resolve));
  });
  const { result, rerender } = renderHook(({ list }) => useSizeEstimate(list, false), { initialProps: { list: apps } });
  // Worker A dies on the busy error, worker B hangs on B.
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(2));
  await finishOutstanding(); // B resolves; worker B continues to C
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(3));
  await finishOutstanding();
  await waitFor(() => expect(result.current.estimating).toBe(false));
  // The busy path stayed out of the cache (no frozen kb:0) and out of the
  // explicit-stop list, so a later refresh estimates it again.
  expect(result.current.sizeOf(apps[0]!)).toBe(0);
  rerender({ list: [...apps] });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenLastCalledWith("C:\\A"));
  await act(async () => complete.splice(0).forEach(resolve => resolve({ kb: 7, capped: false })));
  await waitFor(() => expect(result.current.sizeOf(apps[0]!)).toBe(7));
});

it("generic estimate failures are not cached — next refresh retries", async () => {
  let failing = true;
  native.estimateDirSizeKb.mockImplementation((path: string) =>
    path === "C:\\B" && failing
      ? Promise.reject(new Error("io error"))
      : Promise.resolve({ kb: 10, capped: false }),
  );
  const { result, rerender } = renderHook(({ list }) => useSizeEstimate(list, false), { initialProps: { list: apps } });
  await waitFor(() => expect(result.current.estimating).toBe(false));
  expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(3);
  expect(result.current.sizeOf(apps[1]!)).toBe(0);
  failing = false;
  rerender({ list: [...apps] });
  await waitFor(() => expect(native.estimateDirSizeKb).toHaveBeenCalledTimes(4));
  expect(native.estimateDirSizeKb).toHaveBeenLastCalledWith("C:\\B");
  await waitFor(() => expect(result.current.sizeOf(apps[1]!)).toBe(10));
});
