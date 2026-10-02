// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ToastHost } from "../components/ui/ToastHost";
import { AppRow } from "../components/AppRow";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import type { InstalledApp } from "../types";

vi.mock("../components/AppIcon", () => ({ AppIcon: () => null }));
afterEach(() => { cleanup(); toast.clear(); vi.useRealTimers(); });

it("announces new toasts through one persistent live owner and keeps errors readable", () => {
  vi.useFakeTimers();
  const { container } = render(<ToastHost />);
  const live = container.querySelector('[aria-live="polite"]');
  act(() => { toast.success("Saved"); toast.error("Failure", { detail: "Long actionable reason" }); });
  expect(container.querySelector('[aria-live="polite"]')).toBe(live);
  expect(container.querySelectorAll('[aria-live], [role="alert"], [role="status"]')).toHaveLength(1);
  expect(screen.getByText("Long actionable reason").className).not.toContain("ell");
  act(() => { vi.advanceTimersByTime(6999); });
  expect(screen.queryByText("Saved")).toBeNull();
  expect(screen.getByText("Failure")).toBeTruthy();
  act(() => { vi.advanceTimersByTime(1); });
  expect(screen.queryByText("Failure")).toBeNull();
});

it("returns focus to the row trigger before invoking a menu action", () => {
  const app: InstalledApp = { name: "Demo", publisher: "Vendor", version: "1", registry_key: "demo", install_location: "", uninstall_string: "", quiet_uninstall_string: "", source: "Registry", estimated_size_kb: 0, install_date: "", display_icon: "" };
  let focusAtAction: Element | null = null;
  const analyze = vi.fn(() => { focusAtAction = document.activeElement; });
  render(<table><tbody><AppRow app={app} rowKey="demo" selected={false} checked={false} sizeText="" sizeKb={0} uninstalling={false} index={0}
    onSelect={vi.fn()} onUninstall={vi.fn()} onAnalyze={analyze} onForceClean={vi.fn()} onIgnoreApp={vi.fn()} onIgnorePub={vi.fn()}
    onToggleMulti={vi.fn()} onEnsureSelected={vi.fn()} /></tbody></table>);
  const trigger = screen.getByRole("button", { name: t().rowMore });
  fireEvent.click(trigger);
  const item = screen.getByRole("menuitem", { name: t().rowAnalyze });
  item.focus();
  fireEvent.click(item);
  expect(analyze).toHaveBeenCalledWith(app);
  expect(focusAtAction).toBe(trigger);
  expect(screen.queryByRole("menu")).toBeNull();
});
