// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useState } from "react";
import { DirectoryWorkspace, type FolderInspection } from "../components/DirectoryWorkspace";
import { t } from "../i18n";
import { api } from "../lib/api";
import type { FolderReferences, InstalledApp } from "../types";

vi.mock("../lib/api", () => ({
  api: {
    appIconData: vi.fn(async () => null),
    beginAssociationScan: vi.fn(async () => 4),
    cancelAssociationScan: vi.fn(async () => true),
    associationScanProgress: vi.fn(async () => null),
    inspectFolderReferences: vi.fn(),
    listLocalDrives: vi.fn(),
    listTopDirSizes: vi.fn(),
  },
}));

const app: InstalledApp = {
  name: "Editor", version: "2", publisher: "Example", install_location: "D:\\Apps\\Editor",
  uninstall_string: "uninstall.exe", quiet_uninstall_string: "", source: "HKLM64",
  registry_key: "Editor.2", estimated_size_kb: 180, install_date: "", display_icon: "",
};

function Fixture({ radar = true, path }: { radar?: boolean; path?: string }) {
  const [inspection, setInspection] = useState<FolderInspection | null>(path ? { path } : null);
  return <DirectoryWorkspace
    radar={radar}
    inspection={inspection}
    onInspect={setInspection}
    onBack={() => setInspection(null)}
    onClose={() => setInspection(null)}
    apps={[app]}
    onOpenSoftware={vi.fn()}
  />;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("keeps the inspected references and disk radar when returning from app details", async () => {
  const L = t();
  vi.mocked(api.listLocalDrives).mockResolvedValue([{ letter: "D", is_system: true, total_gb: 10, free_gb: 2 }]);
  vi.mocked(api.listTopDirSizes).mockResolvedValue([{
    path: app.install_location, name: "Editor", parent: "D:\\Apps", size_kb: 41_000, capped: true,
  }]);
  vi.mocked(api.inspectFolderReferences).mockResolvedValue({
    references: [
      { source: "installation", name: app.name, app_source: app.source, location: app.registry_key, target: app.install_location },
      { source: "service", name: "Editor updater", location: "Services\\Editor", target: "D:\\Apps\\Editor\\updater.exe" },
    ],
    incomplete: ["shortcut"],
    omitted: 0,
  });
  render(<Fixture />);
  fireEvent.click(await screen.findByRole("button", { name: L.folderInspectAction }));
  const inspected = await screen.findByRole("dialog", { name: L.folderInspectTitle });
  expect(await within(inspected).findByText(L.folderInspectPartial(L.folderInspectShortcut))).toBeTruthy();
  expect(within(inspected).getByText("D:\\Apps\\Editor\\updater.exe")).toBeTruthy();
  expect(api.inspectFolderReferences).toHaveBeenCalledWith(app.install_location, 4);
  const openDetail = within(inspected).getByRole("button", { name: L.folderInspectShowApp });
  fireEvent.click(openDetail);
  const detail = screen.getByRole("dialog", { name: L.folderInspectShowApp });
  expect(within(detail).getByRole("button", { name: L.detailInspect })).toBeTruthy();
  expect(within(detail).getByRole("button", { name: L.detailUninstall })).toBeTruthy();
  fireEvent.click(within(detail).getByRole("button", { name: L.folderInspectBack }));
  expect(document.activeElement).toBe(openDetail);
  expect(screen.getByText("D:\\Apps\\Editor\\updater.exe")).toBeTruthy();
  expect(api.inspectFolderReferences).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: L.folderInspectRadar }));
  expect(screen.getByRole("button", { name: L.folderInspectAction })).toBeTruthy();
  expect(api.listTopDirSizes).toHaveBeenCalledTimes(1);
});

it("does not display a late result from the previous path", async () => {
  const L = t();
  let finish!: (value: FolderReferences) => void;
  vi.mocked(api.inspectFolderReferences)
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValueOnce({ references: [], incomplete: [], omitted: 0 });
  render(<Fixture radar={false} path="D:\\Apps\\Old" />);
  await waitFor(() => expect(api.inspectFolderReferences).toHaveBeenCalledTimes(1));
  fireEvent.change(screen.getByRole("textbox", { name: L.folderInspectPath }), { target: { value: "D:\\Apps\\New" } });
  fireEvent.click(screen.getByRole("button", { name: L.folderInspectRun }));
  expect(await screen.findByText(L.folderInspectEmpty)).toBeTruthy();
  await act(async () => finish({
    references: [{ source: "service", name: "Old updater", location: "Old", target: "D:\\Apps\\Old\\updater.exe" }],
    incomplete: [],
    omitted: 0,
  }));
  expect(screen.queryByText("Old updater")).toBeNull();
  expect(screen.getByText(L.folderInspectEmpty)).toBeTruthy();
});

it("shows only the completed sources after a cancelled check and clears them on retry", async () => {
  const L = t();
  let finish!: (value: { checked: ["installation"] }) => void;
  vi.mocked(api.inspectFolderReferences)
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValueOnce({ references: [], incomplete: [], omitted: 0 });
  render(<Fixture radar={false} path={app.install_location} />);
  await waitFor(() => expect(api.inspectFolderReferences).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole("button", { name: L.cancelScan }));
  await act(async () => finish({ checked: ["installation"] }));
  expect(await screen.findByText((_text, element) => element?.getAttribute("role") === "status"
    && element.textContent?.includes(L.folderInspectChecked(L.folderInspectInstallation)) === true)).toBeTruthy();
  expect(screen.queryByText(L.folderInspectEmpty)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: L.folderInspectRun }));
  expect(await screen.findByText(L.folderInspectEmpty)).toBeTruthy();
  expect(screen.queryByText(/已查完|Completed sources/)).toBeNull();
});
