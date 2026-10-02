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
