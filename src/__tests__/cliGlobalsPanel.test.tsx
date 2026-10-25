// @vitest-environment jsdom
// Read-only global CLI package inventory panel.
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { CliGlobalsPanel } from "../components/CliGlobalsPanel";
import { api } from "../lib/api";
import { toast } from "../lib/toast";
import { t } from "../i18n";

vi.mock("../lib/api", () => ({
  api: { cliGlobals: vi.fn(), openPath: vi.fn() },
}));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("groups the inventory by manager and opens the root folder", async () => {
  vi.mocked(api.cliGlobals).mockResolvedValue({
    items: [
      { manager: "npm", name: "@scope/tool", version: "1.0.0", path: "C:\\npm\\node_modules\\@scope\\tool", size_kb: 900, size_capped: false },
      { manager: "npm", name: "typescript", version: "5.6.0", path: "C:\\npm\\node_modules\\typescript", size_kb: 40_000, size_capped: false },
      { manager: "pnpm", name: "rimraf", version: null, path: "C:\\pnpm\\rimraf.cmd", size_kb: 0, size_capped: false },
    ],
    omitted: 0,
    notes: ["yarn"],
  });
  render(<CliGlobalsPanel onClose={vi.fn()} />);
  const L = t();
  expect(await screen.findByText("typescript")).toBeTruthy();
  expect(screen.getByText("@scope/tool")).toBeTruthy();
  expect(screen.getByText("rimraf")).toBeTruthy();
  expect(screen.getByText(L.cliNoRoot("yarn"))).toBeTruthy();
  // No size column for pnpm shims.
  expect(screen.queryByText("0 B")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: L.cliOpenRoot("npm") }));
  await waitFor(() => expect(api.openPath).toHaveBeenCalledWith("C:\\npm"));
});

it("shows the empty state and the read-only note", async () => {
  vi.mocked(api.cliGlobals).mockResolvedValue({ items: [], omitted: 0, notes: ["npm", "pnpm", "yarn"] });
  render(<CliGlobalsPanel onClose={vi.fn()} />);
  expect(await screen.findByText(t().cliEmpty)).toBeTruthy();
  expect(screen.getByText(t().cliNote)).toBeTruthy();
});

it("surfaces load errors as an alert", async () => {
  vi.mocked(api.cliGlobals).mockRejectedValue(new Error("boom"));
  render(<CliGlobalsPanel onClose={vi.fn()} />);
  await waitFor(() => expect(toast.error).not.toHaveBeenCalled());
  expect(screen.getByRole("alert").textContent).toContain("boom");
});
