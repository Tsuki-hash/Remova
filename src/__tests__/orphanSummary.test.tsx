// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { OrphanPage } from "../components/OrphanPage";
import { t } from "../i18n";

vi.mock("../lib/api", () => ({ api: { orphanScan: async () => [{
  path: "fixture-dir", kind: "dir", score: 90, confidence: "confirmed", risk: "low",
  reason: "fixture", evidence: [], shared: false, user_data: false, size_kb: 0,
}] } }));
vi.mock("../lib/toast", () => ({ toast: { info: vi.fn(), success: vi.fn(), error: vi.fn() } }));
afterEach(cleanup);

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
