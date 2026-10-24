// @vitest-environment jsdom
// M1 (SOP §9.7): broken MSI records are detected read-only and the first
// action is re-running the vendor uninstaller, followed by a rescan.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { BrokenInstallsPanel } from "../components/BrokenInstallsPanel";
import { setLang, t } from "../i18n";
import type { BrokenInstall } from "../types";

const row: BrokenInstall = {
  guid: "{11111111-1111-1111-1111-111111111111}",
  reason: "broken_install_missing_dir",
  app: {
    name: "Broken App",
    version: "1.2.3",
    publisher: "Vendor",
    install_location: "C:\\Program Files\\Broken App",
    uninstall_string: 'MsiExec.exe /X{11111111-1111-1111-1111-111111111111}',
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: "broken-app",
    estimated_size_kb: 1024,
    install_date: "2024-01-01",
    display_icon: "",
  },
};

const brokenInstalls = vi.fn();
const officialUninstall = vi.fn();
const cleanupBrokenInstall = vi.fn();
const elevateRestart = vi.fn();
const requestConfirmEx = vi.hoisted(() => vi.fn(async () => ({ ok: true, checked: false })));
vi.mock("../lib/api", () => ({
  api: {
    brokenInstalls: (...a: unknown[]) => brokenInstalls(...a),
    officialUninstall: (...a: unknown[]) => officialUninstall(...a),
    cleanupBrokenInstall: (...a: unknown[]) => cleanupBrokenInstall(...a),
    elevateRestart: (...a: unknown[]) => elevateRestart(...a),
  },
}));
vi.mock("../lib/confirm", () => ({ requestConfirmEx }));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));
afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  setLang("zh");
});

const panel = () =>
  render(<BrokenInstallsPanel onClose={vi.fn()} onError={vi.fn()} />);

it("lists a broken MSI record with its missing path and GUID", async () => {
  brokenInstalls.mockResolvedValue([row]);
  panel();
  await screen.findByText("Broken App");
  const L = t();
  expect(screen.getByText(row.app.install_location, { exact: false })).toBeTruthy();
  expect(screen.getByText(row.guid, { exact: false })).toBeTruthy();
  expect(screen.getByText(L.brokenInstallsMissingDir, { exact: false })).toBeTruthy();
});

it("says so when there is nothing to repair", async () => {
  brokenInstalls.mockResolvedValue([]);
  panel();
  await screen.findByText(t().brokenInstallsEmpty);
});

it("re-runs the vendor uninstaller and rescans afterwards", async () => {
  brokenInstalls.mockResolvedValueOnce([row]).mockResolvedValueOnce([]);
  officialUninstall.mockResolvedValue({ ok: true, message: "done", had_command: true });
  panel();
  await screen.findByText("Broken App");
  fireEvent.click(screen.getByRole("button", { name: t().brokenInstallsRerun }));
  await waitFor(() => expect(officialUninstall).toHaveBeenCalledWith(row.app));
  await waitFor(() => expect(brokenInstalls).toHaveBeenCalledTimes(2));
  await screen.findByText(t().brokenInstallsEmpty);
});

it("removes a broken record only after confirmation and reports the backup", async () => {
  const { toast } = await import("../lib/toast");
  brokenInstalls.mockResolvedValueOnce([row]).mockResolvedValueOnce([]);
  cleanupBrokenInstall.mockResolvedValue({
    removed: [row.app.registry_key],
    failed: [],
    backup_dir: "C:\\ProgramData\\Remova\\Backup\\sess",
  });
  panel();
  await screen.findByText("Broken App");
  fireEvent.click(screen.getByRole("button", { name: t().brokenInstallsRemove }));
  await waitFor(() =>
    expect(cleanupBrokenInstall).toHaveBeenCalledWith(
      row.app.name,
      row.app.registry_key,
      row.guid,
    ),
  );
  await waitFor(() => expect(toast.success).toHaveBeenCalled());
  await waitFor(() => expect(brokenInstalls).toHaveBeenCalledTimes(2));
});

it("offers an elevated restart when removal is refused for admin rights", async () => {
  const { toast } = await import("../lib/toast");
  brokenInstalls.mockResolvedValue([row]);
  cleanupBrokenInstall.mockRejectedValue(
    "administrator privileges are required for machine-wide records",
  );
  panel();
  await screen.findByText("Broken App");
  fireEvent.click(screen.getByRole("button", { name: t().brokenInstallsRemove }));
  await waitFor(() => expect(cleanupBrokenInstall).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalledTimes(2));
  expect(requestConfirmEx).toHaveBeenLastCalledWith(
    expect.objectContaining({ message: t().brokenInstallsNeedAdmin }),
  );
  await waitFor(() => expect(elevateRestart).toHaveBeenCalledTimes(1));
  expect(toast.error).not.toHaveBeenCalled();
});

it("keeps standard privileges when the elevation prompt is declined", async () => {
  requestConfirmEx
    .mockResolvedValueOnce({ ok: true, checked: false })
    .mockResolvedValueOnce({ ok: false, checked: false });
  brokenInstalls.mockResolvedValue([row]);
  cleanupBrokenInstall.mockRejectedValue(
    "administrator privileges are required for machine-wide records",
  );
  panel();
  await screen.findByText("Broken App");
  fireEvent.click(screen.getByRole("button", { name: t().brokenInstallsRemove }));
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalledTimes(2));
  expect(elevateRestart).not.toHaveBeenCalled();
});

it("reports other removal failures without the elevation prompt", async () => {
  const { toast } = await import("../lib/toast");
  brokenInstalls.mockResolvedValue([row]);
  cleanupBrokenInstall.mockRejectedValue("boom");
  panel();
  await screen.findByText("Broken App");
  fireEvent.click(screen.getByRole("button", { name: t().brokenInstallsRemove }));
  await waitFor(() => expect(toast.error).toHaveBeenCalled());
  expect(toast.error).toHaveBeenCalledWith(
    t().brokenInstallsRemoveFailed("操作失败：boom"),
  );
  expect(elevateRestart).not.toHaveBeenCalled();
});
