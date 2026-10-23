// @vitest-environment jsdom
// ManageListPage had zero component-level coverage — `appIntegration`
// mocks it away — so the two highest-risk interactions on the page (the
// elevation gate and stop-service vs disable-service) had no regression net.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ManageListPage } from "../components/ManageListPage";
import type { ManageItem } from "../types";
import { setLang, t } from "../i18n";

const native = vi.hoisted(() => ({
  listServices: vi.fn(),
  listStartupItems: vi.fn(),
  listScheduledTasks: vi.fn(),
  isElevated: vi.fn(),
  elevateRestart: vi.fn(),
  setServiceRunning: vi.fn(),
  setServiceStartDisabled: vi.fn(),
  setStartupEnabled: vi.fn(),
  setTaskEnabled: vi.fn(),
  beginAssociationScan: vi.fn(async () => 401),
  cancelAssociationScan: vi.fn(async () => true),
  associationScanProgress: vi.fn(async () => null),
  openPath: vi.fn(async () => {}),
}));
vi.mock("../lib/api", () => ({ api: native }));
// Confirm is a module-level store; drive it through the real dialog so the
// danger/confirm labels the page passes are exercised end to end.
vi.mock("../lib/confirm", () => ({ requestConfirm: vi.fn(async () => true) }));
vi.mock("../lib/toast", () => ({ toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() } }));

const { requestConfirm } = await import("../lib/confirm");
const { toast } = await import("../lib/toast");

// The page drives @tanstack/react-virtual directly (no jsdom fallback like
// VirtualList has), and jsdom reports zero layout, so give the virtualizer a
// measurable viewport or it renders zero rows.
const realRect = Element.prototype.getBoundingClientRect;
const RECT = { top: 0, left: 0, bottom: 600, right: 320, width: 320, height: 600, x: 0, y: 0, toJSON: () => ({}) } as DOMRect;
beforeEach(() => {
  Element.prototype.getBoundingClientRect = () => RECT;
  // `observeElementRect` sizes the scroll box from offsetHeight, not the rect.
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 320 });
});
afterEach(() => {
  Element.prototype.getBoundingClientRect = realRect;
  delete (HTMLElement.prototype as unknown as Record<string, unknown>).offsetHeight;
  delete (HTMLElement.prototype as unknown as Record<string, unknown>).offsetWidth;
});

afterEach(() => { cleanup(); vi.clearAllMocks(); setLang("zh"); });
beforeEach(() => {
  setLang("zh");
  native.isElevated.mockResolvedValue(true);
  // Reset the confirm store's implementation: a queued `...Once` value from an
  // earlier test would otherwise decide this test's outcome.
  vi.mocked(requestConfirm).mockReset();
  vi.mocked(requestConfirm).mockResolvedValue(true);
});

const service = (over: Partial<ManageItem> = {}): ManageItem => ({
  name: "Acme Cache Service",
  detail: "C:\\Program Files\\Acme\\cache.exe",
  location: "service",
  enabled: true,
  kind: "service",
  running: true,
  start_type: "auto",
  ...over,
});

const renderServices = (items: ManageItem[]) => {
  native.listServices.mockResolvedValue(items);
  return render(<ManageListPage tab="services" title={t().navServices} onError={vi.fn()} />);
};

it("gates every mutating action behind elevation and offers the restart prompt", async () => {
  native.isElevated.mockResolvedValue(false);
  // Only the elevation offer is answered; the mutating call must not be retried.
  vi.mocked(requestConfirm).mockResolvedValueOnce(true);
  native.elevateRestart.mockResolvedValue(undefined);
  renderServices([service()]);
  fireEvent.click(await screen.findByRole("button", { name: `${t().manageStop}: Acme Cache Service` }));
  await waitFor(() => expect(native.isElevated).toHaveBeenCalled());
  // Not elevated and the user accepted the offer → the app restarts elevated and
  // the mutating call is NOT retried in this process.
  await waitFor(() => expect(native.elevateRestart).toHaveBeenCalledOnce());
  expect(native.setServiceRunning).not.toHaveBeenCalled();
  expect(toast.error).not.toHaveBeenCalled();
});

it("refuses to touch a service when the elevation offer is declined", async () => {
  native.isElevated.mockResolvedValue(false);
  vi.mocked(requestConfirm).mockResolvedValueOnce(false);
  renderServices([service()]);
  fireEvent.click(await screen.findByRole("button", { name: `${t().manageStop}: Acme Cache Service` }));
  await waitFor(() => expect(native.elevateRestart).not.toHaveBeenCalled());
  expect(native.setServiceRunning).not.toHaveBeenCalled();
});

// The whole point of the two buttons: stopping a service is a runtime action,
// disabling it changes the start type. They must not share a code path or copy.
it("separates stopping a service from disabling its start type", async () => {
  renderServices([service()]);
  const stop = await screen.findByRole("button", { name: `${t().manageStop}: Acme Cache Service` });
  const disable = screen.getByRole("button", { name: `${t().manageDisable}: Acme Cache Service` });

  fireEvent.click(stop);
  await waitFor(() => expect(native.setServiceRunning).toHaveBeenCalledWith("Acme Cache Service", false));
  expect(native.setServiceStartDisabled).not.toHaveBeenCalled();
  // The stop confirmation is its own danger copy, not the disable copy.
  expect(vi.mocked(requestConfirm).mock.calls[0]?.[0]?.message).toBe(t().confirmStopService("Acme Cache Service"));

  vi.mocked(requestConfirm).mockClear();
  native.setServiceRunning.mockClear();
  fireEvent.click(disable);
  await waitFor(() => expect(native.setServiceStartDisabled).toHaveBeenCalledWith("Acme Cache Service", true));
  expect(native.setServiceRunning).not.toHaveBeenCalled();
  expect(vi.mocked(requestConfirm).mock.calls[0]?.[0]?.message).toBe(t().confirmDisableServiceVsStop("Acme Cache Service"));
});

it("starts a stopped service through the same button without disabling it", async () => {
  renderServices([service({ running: false })]);
  const start = await screen.findByRole("button", { name: `${t().manageStartBtn}: Acme Cache Service` });
  fireEvent.click(start);
  await waitFor(() => expect(native.setServiceRunning).toHaveBeenCalledWith("Acme Cache Service", true));
  expect(native.setServiceStartDisabled).not.toHaveBeenCalled();
  expect(vi.mocked(requestConfirm).mock.calls[0]?.[0]?.message).toBe(t().confirmStartService("Acme Cache Service"));
});

it("re-enabling a running start type needs no confirmation at all", async () => {
  renderServices([service({ enabled: false, start_type: "disabled" })]);
  fireEvent.click(await screen.findByRole("button", { name: `${t().manageEnable}: Acme Cache Service` }));
  await waitFor(() => expect(native.setServiceStartDisabled).toHaveBeenCalledWith("Acme Cache Service", false));
  expect(requestConfirm).not.toHaveBeenCalled();
});

it("reports an access-denied mutation and re-offers elevation", async () => {
  native.setServiceRunning.mockRejectedValueOnce(new Error("access is denied"));
  renderServices([service()]);
  fireEvent.click(await screen.findByRole("button", { name: `${t().manageStop}: Acme Cache Service` }));
  await waitFor(() => expect(toast.error).toHaveBeenCalled());
  // isElevated was already true, so no second elevation prompt is needed.
  await act(async () => { await Promise.resolve(); });
  expect(native.elevateRestart).not.toHaveBeenCalled();
});

it("keeps the startup tab on the startup command and never the service command", async () => {
  native.listStartupItems.mockResolvedValue([{
    name: "Acme Agent", detail: "Acme", location: "HKCU\\Run\\AcmeAgent", enabled: true, kind: "startup",
  }]);
  render(<ManageListPage tab="startup" title={t().navStartup} onError={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: `${t().manageDisable}: Acme Agent` }));
  await waitFor(() => expect(native.setStartupEnabled).toHaveBeenCalledWith("HKCU\\Run\\AcmeAgent", false));
  expect(native.setServiceRunning).not.toHaveBeenCalled();
  expect(native.setServiceStartDisabled).not.toHaveBeenCalled();
  expect(vi.mocked(requestConfirm).mock.calls[0]?.[0]?.message).toBe(t().confirmDisableStartup("Acme Agent"));
});

it("shows run state and start type as separate facts on a service row", async () => {
  renderServices([service({ running: false, start_type: "manual" })]);
  await screen.findByRole("button", { name: `${t().manageStartBtn}: Acme Cache Service` });
  expect(screen.getByText(t().manageStatusStopped)).toBeTruthy();
  // "启动类型: 手动" — rendered as its own fact, not folded into the run state.
  expect(screen.getAllByText(new RegExp(`${t().manageStartType}:`)).length).toBeGreaterThan(0);
});
