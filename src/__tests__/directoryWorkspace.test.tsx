// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useState } from "react";
import { DirectoryWorkspace, type FolderInspection } from "../components/DirectoryWorkspace";
import { t } from "../i18n";
import { api } from "../lib/api";
import { getConfirm, settleConfirm } from "../lib/confirm";
import { DISK_RADAR_SNAPSHOT_KEY, snapshotDiskRadar } from "../lib/diskRadarSnapshot";
import type { FolderReferences, InstalledApp, RegenerableItem } from "../types";

vi.mock("../lib/api", () => ({
  api: {
    appIconData: vi.fn(async () => null),
    beginAssociationScan: vi.fn(async () => 4),
    cancelAssociationScan: vi.fn(async () => true),
    associationScanProgress: vi.fn(async () => null),
    inspectFolderReferences: vi.fn(),
    pickReferenceFolder: vi.fn(),
    listLocalDrives: vi.fn(),
    listTopDirSizes: vi.fn(),
    diskRadarVolumeId: vi.fn(async () => "volume-guid"),
    regenerableLayouts: vi.fn(),
    deleteRegenerableLayouts: vi.fn(),
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
  localStorage.clear();
  vi.clearAllMocks();
});

function growingFolder(path: string) {
  const row = { path, name: "Temp", parent: path.slice(0, path.lastIndexOf("\\")), size_kb: 100 };
  const baseline = snapshotDiskRadar("volume-guid", "system:v1", [row]);
  localStorage.setItem(DISK_RADAR_SNAPSHOT_KEY, JSON.stringify(baseline));
  vi.mocked(api.listLocalDrives).mockResolvedValue([{ letter: "D", is_system: true, total_gb: 10, free_gb: 2 }]);
  vi.mocked(api.listTopDirSizes).mockResolvedValue([{ ...row, size_kb: 200 }]);
  vi.mocked(api.inspectFolderReferences).mockResolvedValue({ references: [], incomplete: ["service"], omitted: 0 });
}

const cache: RegenerableItem = {
  path: "D:\\Windows\\Temp", reason: "win_temp", deletable: true,
  size_kb: 180, block_reason: null, size_capped: false,
};

it("opens a fresh, unselected cache preview and restores focus and scroll on both returns", async () => {
  const L = t();
  growingFolder(cache.path);
  vi.mocked(api.regenerableLayouts)
    .mockResolvedValueOnce({ items: [cache], omitted: 0 })
    .mockResolvedValueOnce({ items: [{ ...cache, size_kb: 160, size_capped: true },
      { ...cache, path: "D:\\Users\\Example\\AppData\\Local\\Temp" }], omitted: 0 });
  render(<Fixture />);
  const growth = await screen.findByRole("button", { name: `${L.diskRadarInspectGrowth}: ${cache.path}` });
  const dialog = screen.getByRole("dialog", { name: L.diskRadarShortcut });
  dialog.scrollTo = (options?: ScrollToOptions | number) => {
    dialog.scrollTop = typeof options === "object" ? options.top ?? 0 : 0;
  };
  expect(api.regenerableLayouts).not.toHaveBeenCalled();
  dialog.scrollTop = 120;
  fireEvent.click(growth);
  const shortcut = await screen.findByRole("button", { name: L.diskRadarCacheOpen });
  expect(api.regenerableLayouts).toHaveBeenNthCalledWith(1, cache.path);
  dialog.scrollTop = 55;
  fireEvent.click(shortcut);

  const target = screen.getByRole("dialog", { name: L.regenTitle });
  const selection = await within(target).findByRole<HTMLInputElement>("checkbox", { name: cache.path });
  expect(selection.checked).toBe(false);
  expect(within(target).getAllByRole("checkbox")).toHaveLength(1);
  const clean = within(target).getByRole<HTMLButtonElement>("button", { name: L.regenClean });
  expect(clean.disabled).toBe(true);
  expect(within(target).getByText(L.diskRadarCacheSizeFloor("160 KB"))).toBeTruthy();
  expect(api.regenerableLayouts).toHaveBeenNthCalledWith(2, cache.path);
  expect(api.deleteRegenerableLayouts).not.toHaveBeenCalled();

  fireEvent.click(selection);
  fireEvent.click(clean);
  expect(getConfirm()?.message).toContain(cache.path);
  expect(api.deleteRegenerableLayouts).not.toHaveBeenCalled();
  await act(async () => { settleConfirm(false); });
  expect(api.deleteRegenerableLayouts).not.toHaveBeenCalled();

  fireEvent.click(within(target).getByRole("button", { name: L.folderInspectBack }));
  expect(document.activeElement).toBe(shortcut);
  expect(dialog.scrollTop).toBe(55);
  fireEvent.click(screen.getByRole("button", { name: L.folderInspectRadar }));
  expect(document.activeElement).toBe(growth);
  expect(dialog.scrollTop).toBe(120);
  expect(api.listTopDirSizes).toHaveBeenCalledTimes(1);
  expect(api.listLocalDrives).toHaveBeenCalledTimes(1);
});

it("does not offer cleanup for unrelated folders even when their names match", async () => {
  const L = t();
  const path = "D:\\Projects\\Temp";
  growingFolder(path);
  vi.mocked(api.regenerableLayouts).mockResolvedValueOnce({ items: [cache], omitted: 0 });
  render(<Fixture />);
  fireEvent.click(await screen.findByRole("button", { name: `${L.diskRadarInspectGrowth}: ${path}` }));
  expect(await screen.findByText(L.diskRadarCacheUnavailable)).toBeTruthy();
  expect(await screen.findByText(L.folderInspectPartial(L.folderInspectService))).toBeTruthy();
  expect(screen.getByRole("button", { name: L.openLocation })).toBeTruthy();
  expect(screen.queryByRole("button", { name: L.diskRadarCacheOpen })).toBeNull();
  expect(api.deleteRegenerableLayouts).not.toHaveBeenCalled();
});

it("does not carry a previously eligible cache into a preview that cannot verify it", async () => {
  const L = t();
  growingFolder(cache.path);
  vi.mocked(api.regenerableLayouts).mockResolvedValueOnce({ items: [cache], omitted: 0 })
    .mockResolvedValueOnce({ items: [], omitted: 0 });
  render(<Fixture />);
  fireEvent.click(await screen.findByRole("button", { name: `${L.diskRadarInspectGrowth}: ${cache.path}` }));
  fireEvent.click(await screen.findByRole("button", { name: L.diskRadarCacheOpen }));
  const target = within(screen.getByRole("dialog", { name: L.regenTitle }));
  expect(await target.findByText(L.diskRadarCacheUnavailable)).toBeTruthy();
  expect(target.queryByRole("checkbox")).toBeNull();
  expect(target.queryByRole("button", { name: L.regenClean })).toBeNull();
  expect(api.deleteRegenerableLayouts).not.toHaveBeenCalled();
});

it("restores the reference trigger and the original radar focus and scroll position", async () => {
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
  const radarAction = await screen.findByRole("button", { name: L.folderInspectAction });
  const radarDialog = screen.getByRole("dialog", { name: L.diskRadarShortcut });
  radarDialog.scrollTo = (options?: ScrollToOptions | number, y?: number) => {
    radarDialog.scrollTop = (typeof options === "object" ? options.top : y) ?? 0;
  };
  radarAction.focus();
  radarDialog.scrollTop = 240;
  fireEvent.click(radarAction);
  const inspected = await screen.findByRole("dialog", { name: L.folderInspectTitle });
  inspected.scrollTop = 0;
  expect(await within(inspected).findByText(L.folderInspectPartial(L.folderInspectShortcut))).toBeTruthy();
  expect(within(inspected).getByText("D:\\Apps\\Editor\\updater.exe")).toBeTruthy();
  expect(api.inspectFolderReferences).toHaveBeenCalledWith(app.install_location, 4);
  const openDetail = within(inspected).getByRole("button", { name: L.folderInspectShowApp });
  openDetail.focus();
  inspected.scrollTop = 135;
  fireEvent.click(openDetail);
  const detail = screen.getByRole("dialog", { name: L.folderInspectShowApp });
  expect(detail.scrollTop).toBe(0);
  expect(within(detail).getByRole("button", { name: L.detailInspect })).toBeTruthy();
  expect(within(detail).getByRole("button", { name: L.detailUninstall })).toBeTruthy();
  fireEvent.click(within(detail).getByRole("button", { name: L.folderInspectBack }));
  expect(document.activeElement).toBe(openDetail);
  expect(inspected.scrollTop).toBe(135);
  expect(screen.getByText("D:\\Apps\\Editor\\updater.exe")).toBeTruthy();
  expect(api.inspectFolderReferences).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: L.folderInspectRadar }));
  expect(screen.getByRole("button", { name: L.folderInspectAction })).toBe(radarAction);
  expect(document.activeElement).toBe(radarAction);
  expect(radarDialog.scrollTop).toBe(240);
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
  const status = screen.getByRole("status");
  expect(status.textContent).toBe("");
  fireEvent.click(screen.getByRole("button", { name: L.cancelScan }));
  await act(async () => finish({ checked: ["installation"] }));
  await waitFor(() => expect(status.textContent).toContain(L.folderInspectChecked(L.folderInspectInstallation)));
  expect(screen.queryByText(L.folderInspectEmpty)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: L.folderInspectRun }));
  expect(await screen.findByText(L.folderInspectEmpty)).toBeTruthy();
  expect(screen.getByRole("status")).toBe(status);
  expect(screen.queryByText(/已查完|Completed sources/)).toBeNull();
});

it("updates the existing live status with incomplete sources and an empty result together", async () => {
  const L = t();
  let finish!: (value: FolderReferences) => void;
  vi.mocked(api.inspectFolderReferences).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  render(<Fixture radar={false} path={app.install_location} />);
  await waitFor(() => expect(api.inspectFolderReferences).toHaveBeenCalledTimes(1));
  const status = screen.getByRole("status");
  expect(status.getAttribute("aria-atomic")).toBe("true");
  expect(status.textContent).toBe("");
  await act(async () => finish({ references: [], incomplete: ["shortcut"], omitted: 0 }));
  expect(screen.getByRole("status")).toBe(status);
  expect(within(status).getByText(L.folderInspectPartial(L.folderInspectShortcut))).toBeTruthy();
  expect(within(status).getByText(L.folderInspectEmpty)).toBeTruthy();
});

it("keeps the current folder when the picker is cancelled and checks the chosen folder", async () => {
  const L = t();
  const path = "D:\\Apps\\Chosen";
  vi.mocked(api.pickReferenceFolder).mockResolvedValueOnce(null).mockResolvedValueOnce(path);
  vi.mocked(api.inspectFolderReferences).mockResolvedValue({ references: [], incomplete: [], omitted: 0 });
  render(<Fixture radar={false} path={app.install_location} />);
  await screen.findByText(L.folderInspectEmpty);
  const choose = screen.getByRole<HTMLButtonElement>("button", { name: L.folderInspectChoose });
  await act(async () => fireEvent.click(choose));
  expect(choose.disabled).toBe(false);
  expect(api.inspectFolderReferences).toHaveBeenCalledTimes(1);
  expect(screen.getByRole<HTMLInputElement>("textbox", { name: L.folderInspectPath }).value).toBe(app.install_location);
  fireEvent.click(choose);
  await waitFor(() => expect(api.inspectFolderReferences).toHaveBeenLastCalledWith(path, 4));
  expect(screen.getByRole<HTMLInputElement>("textbox", { name: L.folderInspectPath }).value).toBe(path);
});
