// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { CopilotPanel } from "../components/CopilotPanel";
import { api } from "../lib/api";
import { t } from "../i18n";
import type { InstalledApp, NlIntent } from "../types";
vi.mock("../lib/api", () => ({ api: { aiParseIntent: vi.fn() } }));
vi.mock("../lib/toast", () => ({ toast: { info: vi.fn(), error: vi.fn() } }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
it("guards repeated Enter and ignores a superseded response", async () => {
  const apps = ["Alpha", "Beta"].map(name => ({ name, publisher: "" } as InstalledApp));
  const resolvers: ((value: NlIntent) => void)[] = [];
  vi.mocked(api.aiParseIntent).mockImplementation(() => new Promise(resolve => resolvers.push(resolve)));
  const apply = vi.fn();
  render(<CopilotPanel apps={apps} aiEnabled onApplyFilter={apply} onAnalyze={vi.fn()}
    onBatch={vi.fn()} onForceClean={vi.fn()} />);
  const input = screen.getByRole("textbox");
  fireEvent.change(input, { target: { value: "Alpha" } });
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(api.aiParseIntent).toHaveBeenCalledTimes(1);
  fireEvent.change(input, { target: { value: "Beta" } });
  fireEvent.keyDown(input, { key: "Enter" });
  const intent = (name: string): NlIntent => ({ action: "list", filter: { name_like: name,
    publisher: null, size_gt_kb: null, installed_after: null }, include_leftovers: false, note: name });
  await act(async () => resolvers[1]!(intent("Beta")));
  await act(async () => resolvers[0]!(intent("Alpha")));
  fireEvent.click(screen.getByRole("button", { name: t().copilotApply }));
  expect(apply).toHaveBeenCalledWith([apps[1]], intent("Beta"));
  fireEvent.change(input, { target: { value: "Changed" } });
  expect(screen.queryByRole("button", { name: t().copilotApply })).toBeNull();
});
it("offers only supported examples while offline", () => {
  render(<CopilotPanel apps={[]} aiEnabled={false} onApplyFilter={vi.fn()} onAnalyze={vi.fn()}
    onBatch={vi.fn()} onForceClean={vi.fn()} />);
  expect(screen.queryByRole("button", { name: t().smartFilterExample3 })).toBeNull();
  expect(screen.queryByRole("button", { name: t().smartFilterExample4 })).toBeNull();
  expect(screen.getByRole("button", { name: t().smartFilterExample2 })).toBeTruthy();
});

// CQ-13: `applyFilter` used to compare `install_date` as a raw string, so every
// registry date that was not already `YYYY-MM-DD` silently matched nothing.
it("applies the AI date filter across every install-date shape", async () => {
  const apps = [
    { name: "Recent", install_date: "2026/01/05", publisher: "" } as InstalledApp,
    { name: "RecentCompact", install_date: "20260105", publisher: "" } as InstalledApp,
    { name: "Old", install_date: "2024-01-15", publisher: "" } as InstalledApp,
    { name: "Undated", install_date: "", publisher: "" } as InstalledApp,
  ];
  let resolveIntent!: (value: NlIntent) => void;
  vi.mocked(api.aiParseIntent).mockImplementation(() => new Promise<NlIntent>(resolve => { resolveIntent = resolve; }));
  const apply = vi.fn();
  render(<CopilotPanel apps={apps} aiEnabled onApplyFilter={apply} onAnalyze={vi.fn()}
    onBatch={vi.fn()} onForceClean={vi.fn()} />);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "recently installed" } });
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
  await act(async () => resolveIntent({ action: "list", filter: { name_like: null, publisher: null,
    size_gt_kb: null, installed_after: "2025-12-11" }, include_leftovers: false, note: "recently installed" }));
  fireEvent.click(screen.getByRole("button", { name: t().copilotApply }));
  expect(apply.mock.calls[0]![0].map((a: InstalledApp) => a.name)).toEqual(["Recent", "RecentCompact"]);
});

// An unparseable cutoff must not silently match every row.
it("fails closed when the AI date filter cannot be parsed", async () => {
  const { toast } = await import("../lib/toast");
  const apps = [{ name: "Any", install_date: "2024-01-15", publisher: "" } as InstalledApp];
  let resolveIntent!: (value: NlIntent) => void;
  vi.mocked(api.aiParseIntent).mockImplementation(() => new Promise<NlIntent>(resolve => { resolveIntent = resolve; }));
  const apply = vi.fn();
  render(<CopilotPanel apps={apps} aiEnabled onApplyFilter={apply} onAnalyze={vi.fn()}
    onBatch={vi.fn()} onForceClean={vi.fn()} />);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "installed recently" } });
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
  await act(async () => resolveIntent({ action: "list", filter: { name_like: null, publisher: null,
    size_gt_kb: null, installed_after: "sometime last spring" }, include_leftovers: false, note: "x" }));
  // Nothing can satisfy an unparseable cutoff, so no list can be applied.
  const applyButton = screen.getByRole("button", { name: t().copilotApply });
  expect(applyButton.hasAttribute("disabled")).toBe(true);
  fireEvent.click(applyButton);
  expect(apply).not.toHaveBeenCalled();
  expect(vi.mocked(toast.info).mock.calls.some(call => call[0] === t().copilotNoMatch)).toBe(true);
});
