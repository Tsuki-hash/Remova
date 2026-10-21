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
vi.mock("../lib/api", () => ({
  api: {
    brokenInstalls: (...a: unknown[]) => brokenInstalls(...a),
    officialUninstall: (...a: unknown[]) => officialUninstall(...a),
  },
}));
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
