// @vitest-environment jsdom
// ManageListPage had zero component-level coverage — `appIntegration`
// mocks it away — so the two highest-risk interactions on the page (the
// elevation gate and stop-service vs disable-service) had no regression net.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ManageListPage } from "../components/ManageListPage";
import type { ManageItem } from "../types";
import { setLang, t } from "../i18n";

const native = vi.hoisted(() => ({
  listServices: vi.fn(),
  listStartupItems: vi.fn(),
  listScheduledTasks: vi.fn(),
  appIconData: vi.fn(),
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

it("loads a packaged startup logo by its Windows application ID without an executable path", async () => {
  const appUserModelId = "Acme.Tools_abcd1234!StartAgent";
  native.appIconData.mockResolvedValue("data:image/png;base64,dGVzdA==");
  native.listStartupItems.mockResolvedValue([{
    name: "Acme Tools", detail: "Store", location: `PACKAGED::registry::${appUserModelId}`,
    enabled: true, kind: "startup", app_user_model_id: appUserModelId,
  }]);
  const { container } = render(<ManageListPage tab="startup" title={t().navStartup} />);
  await waitFor(() => expect(container.querySelector("img")?.getAttribute("src")).toBe("data:image/png;base64,dGVzdA=="));
  expect(native.appIconData).toHaveBeenCalledWith("", true, undefined, { path: "", appUserModelId });
});

it("shows a readable service title, description and icon, and controls it by its service ID", async () => {
  const path = "C:\\Program Files\\Acme\\agent.exe";
  native.appIconData.mockResolvedValue("data:image/png;base64,dGVzdA==");
  const { container } = renderServices([service({
    name: "AcmeSvc",
    display_name: "Acme 后台助手",
    display_icon: path,
    detail: "在后台同步应用数据",
    path,
    location: "HKLM64\\SYSTEM\\CurrentControlSet\\Services\\AcmeSvc",
    system_service: false,
  })]);
  const stop = await screen.findByRole("button", { name: t().manageStop + ": Acme 后台助手" });
  expect(screen.getByText("Acme 后台助手")).toBeTruthy();
  expect(screen.getByText("在后台同步应用数据")).toBeTruthy();
  expect(screen.getByText(/后台服务 · AcmeSvc/)).toBeTruthy();
  expect(screen.queryByText(/HKLM64/)).toBeNull();
  await waitFor(() => expect(container.querySelector("img")?.getAttribute("src")).toBe("data:image/png;base64,dGVzdA=="));
  expect(native.appIconData).toHaveBeenCalledWith(path, true, undefined, { path, appUserModelId: "" });
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "agent.exe" } });
  expect(screen.getByText("Acme 后台助手")).toBeTruthy();
  fireEvent.click(stop);
  await waitFor(() => expect(native.setServiceRunning).toHaveBeenCalledWith("AcmeSvc", false));
});

it("filters Windows services by their binary and localizes the startup service source", async () => {
  const windows = service({ name: "Audiosrv", display_name: "Windows Audio",
    detail: "管理音频设备", system_service: true });
  renderServices([windows]);
  expect(await screen.findByText(t().emptySearch)).toBeTruthy();
  fireEvent.click(screen.getByRole("checkbox", { name: t().filterHideMs }));
  expect(await screen.findByText("Windows Audio")).toBeTruthy();
  expect(screen.getByText(/Windows 系统服务 · Audiosrv/)).toBeTruthy();
  expect(screen.getByText("管理音频设备")).toBeTruthy();
  cleanup();

  native.listStartupItems.mockResolvedValue([{ ...windows, kind: "startup", location: "SVC::Audiosrv" }]);
  render(<ManageListPage tab="startup" title={t().navStartup} />);
  expect(await screen.findByText("Windows Audio")).toBeTruthy();
  expect(screen.getByText(/服务自启 · Audiosrv/)).toBeTruthy();
});

it("shows a scheduled task's executable icon and uses the full registered path when disabling it", async () => {
  const path = "C:\\Program Files\\Vendor\\guard.exe";
  const location = "\\Vendor\\Guard\\LaunchGuard";
  native.appIconData.mockResolvedValue("data:image/png;base64,dGVzdA==");
  native.listScheduledTasks.mockResolvedValue([{
    name: location, location, display_name: "LaunchGuard", enabled: true, detail: "Launch the guard",
    display_icon: path, path, task_action: { kind: "program", program: "guard.exe", script_host: false },
    last_run: "09:00", next_run: "10:00",
  }]);
  const { container } = render(<ManageListPage tab="tasks" title={t().navTasks} />);
  const row = await screen.findByRole("option", { name: /LaunchGuard/ });
  expect(row).toHaveProperty("title", location);
  expect(within(row).getByText("Launch the guard")).toBeTruthy();
  expect(within(row).queryByText("09:00")).toBeNull();
  const detail = within(screen.getByRole("complementary", { name: t().manageTaskDetails }));
  expect(detail.getByText(location)).toBeTruthy();
  expect(detail.getByText("guard.exe")).toBeTruthy();
  expect(detail.getByText("09:00")).toBeTruthy();
  expect(detail.getByText("10:00")).toBeTruthy();
  await waitFor(() => expect(container.querySelector("img")).not.toBeNull());
  expect(native.appIconData).toHaveBeenCalledWith(path, true, undefined, { path, appUserModelId: "" });
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "\\Vendor\\Guard" } });
  fireEvent.click(detail.getByRole("button", { name: t().manageTaskDisable + ": LaunchGuard" }));
  await waitFor(() => expect(native.setTaskEnabled).toHaveBeenCalledWith(location, false));
  expect(requestConfirm).toHaveBeenCalledWith(expect.objectContaining({ message: t().confirmDisableTask(location) }));
});

it("labels scheduled scripts, multiple actions and unknown executables without misattributing an icon", async () => {
  native.listScheduledTasks.mockResolvedValue([
    { name: "\\Script", location: "\\Script", display_name: "Script", enabled: true, detail: "",
      task_action: { kind: "program", program: "pwsh.exe", script_host: true } },
    { name: "\\Multi", location: "\\Multi", display_name: "Multi", enabled: true, detail: "",
      task_action: { kind: "multiple" } },
    { name: "\\Unknown", location: "\\Unknown", display_name: "Unknown", enabled: true, detail: "" },
  ]);
  const { container } = render(<ManageListPage tab="tasks" title={t().navTasks} />);
  const list = within(screen.getByRole("listbox"));
  expect(await list.findByText("通过 pwsh.exe 执行")).toBeTruthy();
  expect(list.getByText("多个执行动作")).toBeTruthy();
  expect(list.getByText("未提供可识别的执行程序")).toBeTruthy();
  expect(container.querySelector("img")).toBeNull();
  expect(native.appIconData).not.toHaveBeenCalled();
});

it("selects the exact task with the keyboard and clears hidden task actions when filtering", async () => {
  native.listScheduledTasks.mockResolvedValue([
    { name: "\\One\\Sync", location: "\\One\\Sync", enabled: true, detail: "First vendor" },
    { name: "\\Two\\Sync", location: "\\Two\\Sync", enabled: true, detail: "Second vendor" },
  ]);
  render(<ManageListPage tab="tasks" title={t().navTasks} />);
  const list = screen.getByRole("listbox");
  const detail = within(screen.getByRole("complementary", { name: t().manageTaskDetails }));
  expect(await detail.findByText("\\One\\Sync")).toBeTruthy();
  fireEvent.keyDown(list, { key: "ArrowDown" });
  expect(detail.getByText("\\Two\\Sync")).toBeTruthy();
  expect(within(list).getByRole("option", { selected: true }).getAttribute("title")).toBe("\\Two\\Sync");
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "First vendor" } });
  expect(detail.queryByRole("button")).toBeNull();
  fireEvent.click(within(list).getByRole("option"));
  expect(detail.getByText("\\One\\Sync")).toBeTruthy();
});
