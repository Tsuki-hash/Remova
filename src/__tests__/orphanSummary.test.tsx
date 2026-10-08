// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { OrphanPage } from "../components/OrphanPage";
import { t } from "../i18n";
import { api } from "../lib/api";
import { toast } from "../lib/toast";

vi.mock("../lib/api", () => ({ api: {
  beginAssociationScan: vi.fn(async () => 100), cancelAssociationScan: vi.fn(async () => {}),
  associationScanProgress: vi.fn(async () => null), orphanScan: vi.fn(async () => [{
  path: "fixture-dir", kind: "dir", score: 90, confidence: "confirmed", risk: "low",
  reason: "fixture", evidence: [], shared: false, user_data: false, size_kb: 0,
}]) } }));
vi.mock("../lib/toast", () => ({ toast: { info: vi.fn(), success: vi.fn(), error: vi.fn() } }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("keeps orphan statistics read-only and its existing selection action working", async () => {
  render(<OrphanPage onLastReport={vi.fn()} />);
  expect(await screen.findByText(`${t().bucketSafe} (1)`)).toBeTruthy();
  expect(screen.queryByRole("button", { name: t().conclusionCleanSafe(1) })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: t().clearSelection }));
  expect(screen.getByRole("button", { name: t().cleanup }).hasAttribute("disabled")).toBe(true);
  const selectSafe = screen.getAllByRole("button", { name: t().orphanSelectSafe })[0];
  if (!selectSafe) throw new Error("missing orphan select-safe action");
  fireEvent.click(selectSafe);
  expect(screen.getByRole("button", { name: `${t().cleanup} (1)` }).hasAttribute("disabled")).toBe(false);
});

it("clears old orphan targets and rejects a cancelled late success without claiming zero leftovers", async () => {
  const onError = vi.fn();
  render(<OrphanPage onLastReport={vi.fn()} onError={onError} />);
  await screen.findByText(`${t().bucketSafe} (1)`);
  let finish!: (list: Awaited<ReturnType<typeof api.orphanScan>>) => void;
  vi.mocked(api.orphanScan).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  fireEvent.click(screen.getByRole("button", { name: t().orphanScan }));
  await waitFor(() => expect(api.orphanScan).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole("button", { name: `${t().cleanup} (1)` })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: t().cancelScan }));
  await act(async () => { finish([]); });
  expect(await screen.findByRole("status")).toHaveProperty("textContent", t().scanCancelledIncomplete);
  expect(screen.queryByText(t().orphanScanEmpty)).toBeNull();
  expect(toast.success).toHaveBeenCalledTimes(1);
  expect(onError).not.toHaveBeenCalled();
});
