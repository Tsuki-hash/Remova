// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { ToastHost } from "../components/ui/ToastHost";
import { CloseChoiceHost } from "../components/ui/CloseChoiceHost";
import { AppRow } from "../components/AppRow";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import { requestCloseChoice, settleCloseChoice } from "../lib/closeMode";
import type { InstalledApp } from "../types";

vi.mock("../components/AppIcon", () => ({ AppIcon: () => null }));
afterEach(() => { cleanup(); settleCloseChoice("cancel"); toast.clear(); vi.useRealTimers(); });

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

it("suspends a portaled row menu while the close choice owns the screen", async () => {
  const app: InstalledApp = { name: "Demo", publisher: "Vendor", version: "1", registry_key: "demo", install_location: "", uninstall_string: "", quiet_uninstall_string: "", source: "Registry", estimated_size_kb: 0, install_date: "", display_icon: "" };
  const analyze = vi.fn();
  render(<><CloseChoiceHost /><table><tbody><AppRow app={app} rowKey="demo" selected={false} checked={false} sizeText="" sizeKb={0} uninstalling={false} index={0}
    onSelect={vi.fn()} onUninstall={vi.fn()} onAnalyze={analyze} onForceClean={vi.fn()} onIgnoreApp={vi.fn()} onIgnorePub={vi.fn()}
    onToggleMulti={vi.fn()} onEnsureSelected={vi.fn()} /></tbody></table></>);
  const L = t();
  const trigger = screen.getByRole("button", { name: L.rowMore });
  fireEvent.click(trigger);
  const menu = screen.getByRole("menu");
  const item = within(menu).getByRole("menuitem", { name: L.rowAnalyze });
  expect(document.activeElement).toBe(item);

  let answer!: ReturnType<typeof requestCloseChoice>;
  act(() => { answer = requestCloseChoice(); });
  const dialog = screen.getByRole("dialog", { name: L.closeChoiceTitle });
  const first = within(dialog).getByRole("checkbox");
  const last = within(dialog).getByRole("button", { name: L.closeModeTray });
  expect(document.activeElement).toBe(first);
  expect(Number(menu.style.zIndex)).toBeLessThan(Number(dialog.parentElement?.style.zIndex));
  expect(menu.hasAttribute("inert")).toBe(true);
  expect(screen.queryByRole("menu")).toBeNull();
  fireEvent.click(item);
  expect(analyze).not.toHaveBeenCalled();
  expect(fireEvent.keyDown(first, { key: "Tab", cancelable: true })).toBe(true);
  fireEvent.keyDown(first, { key: "Tab", shiftKey: true, cancelable: true });
  expect(document.activeElement).toBe(last);
  fireEvent.keyDown(last, { key: "Tab", cancelable: true });
  expect(document.activeElement).toBe(first);
  fireEvent.keyDown(first, { key: "Escape" });
  await expect(answer).resolves.toBe("cancel");
  expect(document.activeElement).toBe(item);
  expect(menu.hasAttribute("inert")).toBe(false);
  fireEvent.click(item);
  expect(analyze).toHaveBeenCalledExactlyOnceWith(app);
  expect(document.activeElement).toBe(trigger);
});

it("activates an app row from the keyboard", () => {
  const app: InstalledApp = { name: "KeyboardDemo", publisher: "Vendor", version: "1", registry_key: "demo", install_location: "", uninstall_string: "", quiet_uninstall_string: "", source: "Registry", estimated_size_kb: 0, install_date: "", display_icon: "" };
  const onSelect = vi.fn();
  const { container } = render(<table><tbody><AppRow app={app} rowKey="demo" selected={false} checked={false} sizeText="" sizeKb={0} uninstalling={false} index={0}
    onSelect={onSelect} onUninstall={vi.fn()} onAnalyze={vi.fn()} onForceClean={vi.fn()} onIgnoreApp={vi.fn()} onIgnorePub={vi.fn()}
    onToggleMulti={vi.fn()} onEnsureSelected={vi.fn()} /></tbody></table>);
  const row = container.querySelector("tr[data-index='0']")!;
  expect(row.getAttribute("tabindex")).toBe("0");
  fireEvent.keyDown(row, { key: "Enter" });
  fireEvent.keyDown(row, { key: " " });
  expect(onSelect).toHaveBeenCalledTimes(2);
  expect(onSelect).toHaveBeenCalledWith(app);
});
