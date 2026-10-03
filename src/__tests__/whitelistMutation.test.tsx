// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { WhitelistPanel } from "../components/WhitelistPanel";
import { t } from "../i18n";
import type { IgnoreLists } from "../lib/api";
const { loadIgnore } = vi.hoisted(() => ({ loadIgnore: vi.fn() }));
vi.mock("../lib/api", () => ({ api: { loadIgnore } }));
afterEach(cleanup);
it("waits for the authoritative publisher mutation and never reloads the old list", async () => {
  loadIgnore.mockResolvedValue({ publishers: [], names: [] });
  let resolve!: (next: IgnoreLists) => void;
  const write = vi.fn(() => new Promise<IgnoreLists>(done => { resolve = done; }));
  const changed = vi.fn();
  render(<WhitelistPanel onClose={() => {}} onError={vi.fn()} onIgnorePublisher={write} onListsChange={changed} />);
  await screen.findByText(t().whitelistEmpty);
  const button = screen.getByRole("button", { name: t().ignorePub });
  fireEvent.click(button);
  fireEvent.click(button);
  expect(write).toHaveBeenCalledTimes(1);
  expect((button as HTMLButtonElement).disabled).toBe(true);
  const next = { publishers: ["New Vendor"], names: [] };
  await act(async () => resolve(next));
  expect(screen.getByText("New Vendor")).toBeTruthy();
  expect(changed).toHaveBeenCalledWith(next);
  expect(loadIgnore).toHaveBeenCalledTimes(1);
});
it("reports a rejected mutation and preserves the prior list", async () => {
  loadIgnore.mockResolvedValue({ publishers: ["Existing Vendor"], names: [] });
  const onError = vi.fn();
  render(<WhitelistPanel onClose={() => {}} onError={onError}
    onIgnorePublisher={() => Promise.reject(new Error("write failed"))} />);
  await screen.findByText("Existing Vendor");
  fireEvent.click(screen.getByRole("button", { name: t().ignorePub }));
  await waitFor(() => expect(onError).toHaveBeenCalledTimes(1));
  expect(screen.getByText("Existing Vendor")).toBeTruthy();
});
