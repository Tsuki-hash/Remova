// @vitest-environment jsdom
// render the real App and domain hooks, substituting page surfaces
// and native IO. Assert observable state and API calls across the App boundary.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import { requestBackupSession, useBackupSessionRequest, clearBackupSessionRequest } from "../lib/backupSessionNavigation";
import type { ComponentProps } from "react";
import type { SoftwarePage } from "../components/SoftwarePage";
import type { MorePage } from "../components/MorePage";
import type { OrphanPage } from "../components/OrphanPage";
import type { ManageListPage } from "../components/ManageListPage";
import type { AppDetailPanel } from "../components/AppDetailPanel";
import type { CleanupReport, FullCleanupReport, InstalledApp, ScanResult } from "../types";

const native = vi.hoisted(() => ({
  listApps: vi.fn(), analyze: vi.fn(), dryRun: vi.fn(), fullCleanup: vi.fn(),
  openPath: vi.fn(), elevateRestart: vi.fn(), takePendingAnalyze: vi.fn(),
  ignorePublisher: vi.fn(), ignoreAppName: vi.fn(),
  beginInstallMonitor: vi.fn(), endInstallMonitor: vi.fn(),
  verifyLeftovers: vi.fn(), aiSummarizeReport: vi.fn(),
  checkLatestRelease: vi.fn(), requestConfirmEx: vi.fn(), requestConfirm: vi.fn(),
  drag: undefined as undefined | ((event: { payload: { type: string; paths: string[] } }) => void),
}));
const pages = vi.hoisted(() => ({
  software: undefined as ComponentProps<typeof SoftwarePage> | undefined,
  more: undefined as ComponentProps<typeof MorePage> | undefined,
  orphan: undefined as ComponentProps<typeof OrphanPage> | undefined,
  manage: undefined as ComponentProps<typeof ManageListPage> | undefined,
  detail: undefined as ComponentProps<typeof AppDetailPanel> | undefined,
}));

vi.mock("../lib/api", () => ({ api: {
  ...native,
  beginAssociationScan: vi.fn(async () => 1),
  cancelAssociationScan: vi.fn(async () => {}),
  associationScanProgress: vi.fn(async () => null),
  isElevated: vi.fn().mockResolvedValue(false),
  getAiConfig: vi.fn().mockResolvedValue({ enabled: false }),
  loadIgnore: vi.fn().mockResolvedValue({ publishers: [], names: [] }),
  diskUsage: vi.fn().mockResolvedValue({ free_gb: 10, total_gb: 100, drive: "C:" }),
} }));
vi.mock("../lib/updateCheck", () => ({
  RELEASES_URL: "https://example.com/releases",
  checkLatestRelease: native.checkLatestRelease,
}));
vi.mock("../lib/confirm", () => ({
  requestConfirmEx: native.requestConfirmEx, requestConfirm: native.requestConfirm,
}));
vi.mock("../lib/exportHtmlReport", () => ({ exportHtmlReport: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => { throw new Error("test has no native window"); },
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (cb: typeof native.drag) => { native.drag = cb; return vi.fn(); },
  }),
}));
vi.mock("../components/WindowControls", () => ({ WindowControls: () => null }));
vi.mock("../components/ui/ConfirmHost", () => ({ ConfirmHost: () => null }));
vi.mock("../components/SoftwarePage", () => ({
  SoftwarePage: (props: ComponentProps<typeof SoftwarePage>) => {
    pages.software = props;
    return <section data-testid="software">{props.detailPanel}</section>;
  },
}));
vi.mock("../components/MorePage", () => ({
  MorePage: (props: ComponentProps<typeof MorePage>) => {
    pages.more = props;
    return <section data-testid="more" />;
  },
}));
vi.mock("../components/ManageListPage", () => ({
  ManageListPage: (props: ComponentProps<typeof ManageListPage>) => {
    pages.manage = props;
    return <section data-testid={props.tab} />;
  },
}));
vi.mock("../components/OrphanPage", () => ({
  OrphanPage: (props: ComponentProps<typeof OrphanPage>) => {
    pages.orphan = props;
    return <section data-testid="orphans" />;
  },
}));
vi.mock("../components/AppDetailPanel", () => ({
  AppDetailPanel: (props: ComponentProps<typeof AppDetailPanel>) => {
    pages.detail = props;
    return <aside data-testid="detail" />;
  },
}));

import App from "../App";
import * as i18n from "../i18n";
import { exportHtmlReport } from "../lib/exportHtmlReport";
import { appKey } from "../lib/appKey";
import { appForPath } from "../hooks/useAppNavAssist";
import { toast } from "../lib/toast";
import { saveRescanAfterUninstall } from "../lib/rescanPref";

const demo: InstalledApp = {
  name: "Demo", version: "1", publisher: "Acme", install_location: "C:\\Apps\\Demo",
  uninstall_string: "un.exe", quiet_uninstall_string: "", source: "HKLM64",
  registry_key: "demo", estimated_size_kb: 100, install_date: "", display_icon: "",
};
const other: InstalledApp = { ...demo, name: "Other", registry_key: "other" };
const scan: ScanResult = {
  app_name: demo.name, app_key: appKey(demo),
  items: [{ path: "C:\\Apps\\Demo\\cache", kind: "dir", score: 90,
    confidence: "confirmed", risk: "low", reason: "install", evidence: [] }],
};
const report: FullCleanupReport = {
  app_name: demo.name, dry_run: false, backup_dir: "", uninstall_ok: true,
  uninstall_message: "", deleted: 1, failed: 0, skipped: 0, delayed: 0,
  aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [],
};
const dryReport: CleanupReport = {
  app_name: demo.name, dry_run: true, uninstall_command: [], uninstall_message: "",
  deleted_planned: 1, skipped: 0, errors: [], item_details: [],
};
const software = () => pages.software!;
const detail = () => pages.detail!;
const more = () => pages.more!;

async function mount() {
  const view = render(<App />);
  await screen.findByTestId("software");
  await waitFor(() => expect(software().loading).toBe(false));
  return view;
}
async function nav(id: "startup" | "services" | "tasks" | "orphans" | "more" | "software") {
  const labels = { startup: i18n.t().navStartup, services: i18n.t().navServices,
    tasks: i18n.t().navTasks, orphans: i18n.t().navOrphans,
    more: i18n.t().navMore, software: i18n.t().navSoftware };
  fireEvent.click(screen.getByRole("button", { name: labels[id] }));
  await screen.findByTestId(id);
}
async function analyzeDemo() {
  act(() => software().listAnalyze(demo));
  await waitFor(() => expect(software().scan).toEqual(scan));
}

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  i18n.setLang("zh");
  for (const key of Object.keys(pages) as (keyof typeof pages)[]) pages[key] = undefined;
  native.drag = undefined;
  native.listApps.mockResolvedValue([demo, other]);
  native.analyze.mockResolvedValue(scan);
  native.dryRun.mockResolvedValue(dryReport);
  native.fullCleanup.mockResolvedValue(report);
  native.openPath.mockResolvedValue(undefined);
  native.elevateRestart.mockResolvedValue(undefined);
  native.takePendingAnalyze.mockResolvedValue(null);
  native.checkLatestRelease.mockResolvedValue({ ok: false, reason: "offline" });
  native.requestConfirmEx.mockResolvedValue({ ok: false, checked: false });
  native.requestConfirm.mockResolvedValue(false);
  native.ignorePublisher.mockResolvedValue({ publishers: [demo.publisher] });
  native.ignoreAppName.mockResolvedValue({ names: [demo.name] });
  native.beginInstallMonitor.mockResolvedValue(undefined);
  native.endInstallMonitor.mockResolvedValue({
    diff: { added_files: [scan.items[0]!.path], added_reg_values: [] }, items: scan.items,
  });
  native.verifyLeftovers.mockResolvedValue([]);
  native.aiSummarizeReport.mockResolvedValue(null);
});
afterEach(() => { cleanup(); toast.clear(); vi.restoreAllMocks(); });

describe("App orchestration", () => {
  it("clears old report notes and rejects verification belonging to another report", async () => {
    saveRescanAfterUninstall(false);
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    await mount();
    await nav("more");
    act(() => more().onAiEnabledChange?.(true));
    await nav("software");
    await analyzeDemo();
    native.aiSummarizeReport.mockResolvedValueOnce("Previous summary");
    act(() => software().onCleanup());
    await waitFor(() => expect(software().aiReportNote).toBe("Previous summary"));
    let resolveVerify!: (rows: { path: string; kind: string; still_there: boolean }[]) => void;
    native.verifyLeftovers.mockReturnValueOnce(new Promise(resolve => { resolveVerify = resolve; }));
    act(() => software().onCleanup());
    await waitFor(() => expect(native.verifyLeftovers).toHaveBeenCalledTimes(2));
    await nav("more");
    native.aiSummarizeReport.mockRejectedValueOnce(new Error("summary failed"));
    act(() => more().onLastReport?.({ ...report, app_name: "New tool report" }));
    await act(async () => resolveVerify([{ path: "old-path", kind: "file", still_there: true }]));
    await nav("software");
    expect(software().aiReportNote).toBeNull();
    expect(software().verifyRows).toBeNull();
  });

  it("matches full path boundaries in pending and drag entry points", async () => {
    const adjacent = { ...other, install_location: `${demo.install_location}Bar` };
    const nested = { ...other, name: "Nested", install_location: `${demo.install_location}\\Nested` };
    expect(appForPath([demo, adjacent], `${adjacent.install_location}\\file.exe`)).toEqual(adjacent);
    expect(appForPath([demo, nested], `${nested.install_location}\\file.exe`)).toEqual(nested);
    expect(appForPath([demo, { ...other, install_location: demo.install_location }], demo.install_location)).toBeNull();
    expect(appForPath([demo], "C:\\Apps")).toBeNull();
    native.listApps.mockResolvedValue([demo, adjacent]);
    native.takePendingAnalyze.mockResolvedValueOnce(`${demo.install_location}Extra/app.exe`);
    await mount();
    await waitFor(() => expect(native.takePendingAnalyze).toHaveBeenCalled());
    expect(native.analyze).not.toHaveBeenCalled();
    await waitFor(() => expect(native.drag).toBeTruthy());
    act(() => native.drag!({ payload: { type: "drop", paths: ["C:\\Apps"] } }));
    expect(native.analyze).not.toHaveBeenCalled();
    act(() => native.drag!({ payload: { type: "drop", paths: [`${adjacent.install_location}/app.exe`] } }));
    await waitFor(() => expect(native.analyze).toHaveBeenCalledWith(adjacent, 1));
  });

  it("applies saved AI state and removed ignore rules immediately across pages", async () => {
    await mount();
    act(() => software().listIgnorePublisher(demo));
    await waitFor(() => expect(software().filtered).not.toContainEqual(demo));
    await nav("more");
    act(() => {
      more().onAiEnabledChange?.(true);
      more().onIgnoreListsChange?.({ publishers: [], names: [] });
    });
    await nav("software");
    expect(software().aiEnabled).toBe(true);
    expect(software().filtered).toContainEqual(demo);
    await nav("more");
    act(() => more().onAiEnabledChange?.(false));
    await nav("software");
    expect(software().aiEnabled).toBe(false);
  });

  it("boots, filters categories and toggles multi selection through page callbacks", async () => {
    await mount();
    expect(software().apps).toEqual([demo, other]);
    // The admin chip asks for consent (same dialog as the manage pages)
    // before firing UAC and relaunching.
    native.requestConfirm.mockResolvedValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: i18n.t().nonAdmin }));
    await waitFor(() => expect(native.elevateRestart).toHaveBeenCalledTimes(1));
    act(() => software().onCategory("store"));
    expect(localStorage.getItem("remova_cat")).toBe("store");
    expect(software().filtered).toEqual([]);
    act(() => { software().onCategory("all"); software().toggleMulti(appKey(demo)); });
    expect(software().multi).toEqual(new Set([appKey(demo)]));
    act(() => software().toggleMulti(appKey(demo)));
    expect(software().multi.size).toBe(0);
  });

  it("loads saved language once and keeps it when the theme changes", async () => {
    localStorage.setItem("remova_lang", "en");
    const load = vi.spyOn(i18n, "loadLang");
    await mount();
    expect(i18n.currentLang()).toBe("en");
    const before = document.documentElement.dataset.theme;
    fireEvent.click(screen.getByRole("button", { name: i18n.t().themeToggle }));
    expect(document.documentElement.dataset.theme).not.toBe(before);
    expect(load).toHaveBeenCalledTimes(1);
    expect(i18n.currentLang()).toBe("en");
    fireEvent.click(screen.getByRole("button", { name: i18n.t().langToggle }));
    expect(i18n.currentLang()).toBe("zh");
    expect(screen.getByRole("heading", { name: i18n.t().navSoftware })).toBeTruthy();
  });

  it("routes all lazy pages, shares errors and preserves the latest full report", async () => {
    await mount();
    for (const tab of ["startup", "services", "tasks"] as const) {
      await nav(tab);
      expect(pages.manage?.tab).toBe(tab);
      act(() => pages.manage!.onError?.("manage:broken"));
      expect(screen.getByText("manage:broken")).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: i18n.t().errorDismiss }));
      expect(screen.queryByText("manage:broken")).toBeNull();
    }
    await nav("orphans");
    act(() => pages.orphan!.onLastReport?.(report));
    await nav("more");
    expect(more().lastReport).toEqual(report);
    act(() => more().onExportReport());
    expect(exportHtmlReport).toHaveBeenCalledWith(report, i18n.t());
    act(() => more().onCloseModeChange("quit"));
    expect(more().closeMode).toBe("quit");
    act(() => more().onLastReport?.({ ...report, deleted: 2 }));
    expect(more().lastReport?.deleted).toBe(2);
    act(() => more().onError("tools:broken"));
    expect(screen.getByText("tools:broken")).toBeTruthy();
    act(() => more().onGoSoftware());
    await screen.findByTestId("software");
    expect(software().error).toBe("tools:broken");
  });

  it("does not export without a report and wires manual update checks", async () => {
    await mount();
    await nav("more");
    act(() => more().onExportReport());
    expect(exportHtmlReport).not.toHaveBeenCalled();
    // A failed check surfaces the localized reason only — it must not
    // auto-open the browser (the download CTA is the manual path).
    act(() => more().onCheckUpdate());
    await waitFor(() => expect(native.checkLatestRelease.mock.calls.length).toBeGreaterThanOrEqual(2));
    expect(native.openPath).not.toHaveBeenCalled();
  });

  it("guards list and bucket analysis while scanning, then drills only into matching scans", async () => {
    let finish!: (value: ScanResult) => void;
    native.analyze.mockReturnValueOnce(new Promise<ScanResult>((resolve) => { finish = resolve; }));
    await mount();
    act(() => software().selectApp(demo));
    act(() => detail().onDrillDown?.("registry"));
    expect(software().scanning).toBe(true);
    act(() => { software().listAnalyze(other); detail().onDrillDown?.("registry"); });
    await waitFor(() => expect(native.analyze).toHaveBeenCalledTimes(1));
    await act(async () => { finish(scan); });
    expect(software().kindFilter).toBeNull();
    act(() => detail().onDrillDown?.("registry"));
    expect(software().kindFilter).toBe("registry");
    act(() => software().selectApp(other));
    act(() => detail().onDrillDown?.("programFiles"));
    await waitFor(() => expect(native.analyze).toHaveBeenLastCalledWith(other, 1));
    act(() => detail().onClose());
    expect(software().selected).toBeNull();
    expect(screen.queryByTestId("detail")).toBeNull();
  });

  it("keeps search and sorting across preview return and refresh while clearing scan authorization", async () => {
    await mount();
    await analyzeDemo();
    act(() => {
      software().onQuery("Demo");
      software().sortBy("name");
      software().setUseOfficial(true);
      software().setEvidence("shared");
      software().setAiRisk("high");
      software().setAiReportNote("old");
      software().setSelectedPaths(new Set([scan.items[0]!.path]));
      software().setKindFilter("registry");
      software().setRiskFilter("keep");
      software().setShowBatchSummary(true);
    });
    native.listApps.mockRejectedValueOnce("refresh:broken");
    act(() => software().onBack());
    await waitFor(() => expect(software().error).toContain("refresh:broken"));
 // Returning to the list keeps the drawer selection, but resets the preview.
    expect(software()).toMatchObject({ selected: demo, scan: null, report: null,
      q: "Demo", sortCol: "name", sortDesc: false,
      useOfficial: false, residualFromUninstall: false, evidence: null, aiRisk: null,
      aiReportNote: null, kindFilter: null, riskFilter: null, showBatchSummary: false });
    expect(software().selectedPaths.size).toBe(0);
    await analyzeDemo();
    act(() => software().onBack());
    await waitFor(() => expect(native.listApps).toHaveBeenCalledTimes(3));
    expect(software()).toMatchObject({ q: "Demo", sortCol: "name", sortDesc: false, scan: null });
    expect(software().selectedPaths.size).toBe(0);
  });

  it("warns before unload only while dry-run is active and removes the listener on unmount", async () => {
    let finish!: (value: CleanupReport) => void;
    native.dryRun.mockReturnValueOnce(new Promise<CleanupReport>((resolve) => { finish = resolve; }));
    const view = await mount();
    await analyzeDemo();
    const unload = () => {
      const event = new Event("beforeunload", { cancelable: true });
      window.dispatchEvent(event);
      return event.defaultPrevented;
    };
    expect(unload()).toBe(false);
    act(() => software().onDryRun());
    expect(software().dryRunning).toBe(true);
    expect(unload()).toBe(true);
    await act(async () => { finish(dryReport); });
    expect(unload()).toBe(false);
    const remove = vi.spyOn(window, "removeEventListener");
    view.unmount();
    expect(remove).toHaveBeenCalledWith("beforeunload", expect.any(Function));
  });

  it.each([true, false])("post-cleanup reanalysis follows preference %s", async (enabled) => {
    saveRescanAfterUninstall(enabled);
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    await mount();
    await analyzeDemo();
    act(() => software().setUseOfficial(true));
    act(() => software().onCleanup());
    await waitFor(() => expect(native.fullCleanup).toHaveBeenCalled());
    await waitFor(() => expect(software().dryRunning).toBe(false));
    expect(native.analyze).toHaveBeenCalledTimes(enabled ? 2 : 1);
    expect(software().residualFromUninstall).toBe(false);
  });

  it("protects unload during a batch and releases it when the batch completes", async () => {
    let finish!: (value: FullCleanupReport) => void;
    native.fullCleanup.mockReturnValueOnce(new Promise<FullCleanupReport>((resolve) => { finish = resolve; }));
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    await mount();
    act(() => software().toggleMulti(appKey(demo)));
    act(() => software().batchCleanup());
    await waitFor(() => expect(software().batching).toBe(true));
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    await act(async () => { finish(report); });
    await waitFor(() => expect(software().batching).toBe(false));
    const idle = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(idle);
    expect(idle.defaultPrevented).toBe(false);
    expect(software().multi.size).toBe(0);
    expect(software().showBatchSummary).toBe(true);
  });

  it("wires list and drawer uninstall, force-clean, path and ignore actions", async () => {
    await mount();
    act(() => software().selectApp(demo));
    act(() => detail().onOpenPath?.(demo.install_location));
    await waitFor(() => expect(native.openPath).toHaveBeenCalledWith(demo.install_location));
    act(() => software().listStartUninstall(demo));
    await waitFor(() => expect(native.requestConfirmEx).toHaveBeenCalledTimes(1));
    act(() => detail().onDeepUninstall(demo));
    await waitFor(() => expect(native.requestConfirmEx).toHaveBeenCalledTimes(2));
    act(() => detail().onOfficialOnly(demo));
    await waitFor(() => expect(native.requestConfirm).toHaveBeenCalled());
    act(() => detail().onAnalyze(demo));
    await waitFor(() => expect(software().scan).toEqual(scan));
    act(() => software().listForceClean(demo));
    await waitFor(() => expect(software().busy).toBe(false));
    act(() => detail().onForceClean?.(demo));
    await waitFor(() => expect(software().busy).toBe(false));
    expect(native.analyze).toHaveBeenCalledTimes(3);
    act(() => software().listIgnoreApp(demo));
    await waitFor(() => expect(native.ignoreAppName).toHaveBeenCalledWith(demo.name));
    act(() => software().listIgnorePublisher(demo));
    await waitFor(() => expect(native.ignorePublisher).toHaveBeenCalledWith(demo.publisher));
  });

  it("connects monitor snapshots to cleanup and clears dismissed diffs", async () => {
    await mount();
    await nav("more");
    act(() => more().onMonitorToCleanup());
    expect(native.analyze).not.toHaveBeenCalled();
    act(() => more().onToggleMonitor());
    await waitFor(() => expect(more().monitoring).toBe(true));
    act(() => more().onToggleMonitor());
    await waitFor(() => expect(more().monitorDiff).toBeTruthy());
    act(() => more().onMonitorToCleanup());
    await screen.findByTestId("software");
    expect(software().selected?.source).toBe("Monitor");
    expect(software().scan?.items).toEqual(scan.items);
    await nav("more");
    expect(more().monitorDiff).toBeNull();
    act(() => more().onDismissMonitor());
    expect(more().monitorDiff).toBeNull();
    await act(async () => { await more().onIgnorePublisher(); });
 // Synthetic monitor app has no publisher, so ignore is a no-op.
    expect(native.ignorePublisher).not.toHaveBeenCalled();
  });

  it.each([false, true])("keeps monitor session state truthful after cancelled capture, active=%s", async active => {
    await mount();
    await nav("more");
    if (active) {
      act(() => more().onToggleMonitor());
      await waitFor(() => expect(more().monitoring).toBe(true));
    }
    const capture = active ? native.endInstallMonitor : native.beginInstallMonitor;
    let finish!: (result: unknown) => void;
    capture.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    act(() => more().onToggleMonitor());
    await waitFor(() => expect(capture).toHaveBeenCalledOnce());
    expect(more().monitorSnapshotBusy).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: i18n.t().cancelScan }));
    await act(async () => { finish(active ? { diff: { added_files: ["late-path"], added_reg_values: [] }, items: scan.items } : undefined); });
    expect(await within(screen.getByRole("main")).findByRole("status"))
      .toHaveProperty("textContent", i18n.t().monitorScanCancelled(active));
    expect(more().monitoring).toBe(active);
    expect(more().monitorDiff).toBeNull();
    expect(more().monitorSnapshotBusy).toBe(false);
    expect(native.fullCleanup).not.toHaveBeenCalled();
  });

  it("routes a backup preview request to More without starting a cleanup", async () => {
    const store = renderHook(useBackupSessionRequest);
    await mount();
    act(() => requestBackupSession("123_sample"));
    try {
      await waitFor(() => expect(screen.getByTestId("more")).toBeTruthy());
      expect(store.result.current?.name).toBe("123_sample");
      expect(native.fullCleanup).not.toHaveBeenCalled();
    } finally {
      act(() => clearBackupSessionRequest(store.result.current!.id));
    }
  });

  it("hands pending analyze and native drag-drop into the App scan flow", async () => {
    native.listApps.mockResolvedValue([demo]);
    native.takePendingAnalyze.mockResolvedValueOnce("C:\\Apps\\Demo\\app.exe");
    await mount();
    await waitFor(() => expect(software().scan).toEqual(scan));
    expect(native.analyze).toHaveBeenCalledWith(demo, 1);
    await waitFor(() => expect(native.drag).toBeTruthy());
    act(() => native.drag!({ payload: { type: "drop", paths: ["C:\\Apps\\Demo"] } }));
    await waitFor(() => expect(native.analyze).toHaveBeenCalledTimes(2));
  });
});
