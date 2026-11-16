// @vitest-environment jsdom
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AppIcon } from "../components/AppIcon";
import { api } from "../lib/api";

vi.mock("../lib/api", () => ({ api: { appIconData: vi.fn() } }));
const load = vi.mocked(api.appIconData);

afterEach(() => {
  cleanup();
  load.mockReset();
  vi.restoreAllMocks();
});

function icons(prefix: string, count: number) {
  return Array.from({ length: count }, (_, index) => (
    <AppIcon key={`${prefix}-${index}`} displayIcon={`C:\\${prefix}\\${index}.exe`} />
  ));
}

it("limits native icon reads and reuses confirmed misses across list remounts", async () => {
  const complete: Array<() => void> = [];
  let peak = 0;
  load.mockImplementation(() => new Promise(resolve => {
    complete.push(() => resolve(null));
    peak = Math.max(peak, complete.length);
  }));
  const view = render(<>{icons("missing-icons", 30)}</>);
  for (let done = 0; done < 30; done += 4) {
    await waitFor(() => expect(load).toHaveBeenCalledTimes(Math.min(30, done + 4)));
    await act(async () => { for (const finish of complete.splice(0)) finish(); });
  }
  expect(peak).toBe(4);
  view.unmount();
  for (let index = 0; index < 2; index++) {
    const remounted = render(<>{icons("missing-icons", 30)}</>);
    await act(async () => {});
    remounted.unmount();
  }
  expect(load).toHaveBeenCalledTimes(30);
});

it("drops waiting reads when their rows leave the list", async () => {
  const complete: Array<() => void> = [];
  load.mockImplementation(() => new Promise(resolve => { complete.push(() => resolve(null)); }));
  const old = render(<>{icons("old-list", 8)}</>);
  await waitFor(() => expect(load).toHaveBeenCalledTimes(4));
  old.unmount();
  render(<AppIcon displayIcon={"C:\\visible.exe"} />);
  await act(async () => { for (const finish of complete.splice(0)) finish(); });
  expect(load).toHaveBeenCalledTimes(5);
  expect(load).toHaveBeenLastCalledWith("C:\\visible.exe", false);
  await act(async () => { for (const finish of complete.splice(0)) finish(); });
});

it("retries failed reads immediately and retries confirmed misses after expiry", async () => {
  const clock = vi.spyOn(performance, "now").mockReturnValue(1_000);
  const url = "data:image/png;base64,fixture";
  load.mockRejectedValueOnce(new Error("busy")).mockResolvedValueOnce(url);
  const failed = render(<AppIcon displayIcon={"C:\\busy.exe"} />);
  await waitFor(() => expect(load).toHaveBeenCalledTimes(1));
  failed.unmount();
  const retried = render(<AppIcon displayIcon={"C:\\busy.exe"} />);
  await waitFor(() => expect(retried.container.querySelector("img")?.src).toBe(url));
  retried.unmount();

  load.mockResolvedValueOnce(null).mockResolvedValueOnce(url);
  const missing = render(<AppIcon displayIcon={"C:\\installed-later.exe"} />);
  await waitFor(() => expect(load).toHaveBeenCalledTimes(3));
  missing.unmount();
  clock.mockReturnValue(31_001);
  const installed = render(<AppIcon displayIcon={"C:\\installed-later.exe"} />);
  await waitFor(() => expect(installed.container.querySelector("img")?.src).toBe(url));
  expect(load).toHaveBeenCalledTimes(4);
});
