// @vitest-environment jsdom
// Render the real App and domain hooks, substituting page surfaces
// and native IO. Assert observable state and API calls across the App boundary.
// Page internals are covered by their own real-component suites:
// softwarePageIdentity (SoftwarePage), moreReleases + moreExportReport (MorePage),
// orphanSummary (OrphanPage), manageListPage (ManageListPage).
import { cleanupItemKey } from "../lib/cleanupPlan";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import { requestBackupSession, useBackupSessionRequest, clearBackupSessionRequest } from "../lib/backupSessionNavigation";
import type { ComponentProps } from "react";
import type { SoftwarePage } from "../components/SoftwarePage";
import type { MorePage } from "../components/MorePage";
import { MonitorPanel } from "../components/MonitorPanel";
import type { OrphanPage } from "../components/OrphanPage";
import type { ManageListPage } from "../components/ManageListPage";
import type { AppDetailPanel } from "../components/AppDetailPanel";
import type { CleanupReport, FullCleanupReport, InstalledApp, InstalledAppsSnapshot, ScanResult } from "../types";

const native = vi.hoisted(() => ({
  listApps: vi.fn(), analyze: vi.fn(), dryRun: vi.fn(), fullCleanup: vi.fn(), officialUninstall: vi.fn(),
  orphanScan: vi.fn(), beginCleanupOperation: vi.fn(), cancelCleanup: vi.fn(),
  openPath: vi.fn(), elevateRestart: vi.fn(), takePendingAnalyze: vi.fn(),
  ignorePublisher: vi.fn(), ignoreAppName: vi.fn(),
  beginInstallMonitor: vi.fn(), endInstallMonitor: vi.fn(),
  installMonitorStatus: vi.fn(), discardInstallMonitorRecovery: vi.fn(),
  verifyLeftovers: vi.fn(), aiSummarizeReport: vi.fn(),
  checkLatestRelease: vi.fn(), requestConfirmEx: vi.fn(), requestConfirm: vi.fn(),
  showRealSoftwarePage: false,
  showRealOrphanPage: false,
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
  auditProfileRoot: vi.fn(async () => "C:\\Users\\Fixture"),
  setTrayLanguage: vi.fn(async () => {}),
  beginAssociationScan: vi.fn(async () => 1),
  cancelAssociationScan: vi.fn(async () => {}),
  associationScanProgress: vi.fn(async () => null),
  inspectFolderReferences: vi.fn(async () => ({ references: [], incomplete: [], omitted: 0 })),
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
  getConfirm: () => null,
}));
vi.mock("../lib/exportHtmlReport", () => ({ exportHtmlReport: vi.fn() }));
const pendingAnalysisEvent = vi.hoisted(() => ({
  listener: undefined as undefined | (() => void),
}));
vi.mock("@tauri-apps/api/event", async (importOriginal) => ({
  ...await importOriginal<typeof import("@tauri-apps/api/event")>(),
  listen: async (event: string, handler: () => void) => {
    if (event === "pending-analyze") pendingAnalysisEvent.listener = handler;
    return () => { if (event === "pending-analyze") pendingAnalysisEvent.listener = undefined; };
  },
}));
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
vi.mock("../components/SoftwarePage", async (importOriginal) => {
  const { SoftwarePage: RealSoftwarePage } = await importOriginal<typeof import("../components/SoftwarePage")>();
  return {
    SoftwarePage: (props: ComponentProps<typeof SoftwarePage>) => {
      pages.software = props;
      return <section data-testid="software">
        {native.showRealSoftwarePage ? <RealSoftwarePage {...props} /> : props.detailPanel}
      </section>;
    },
  };
});
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
vi.mock("../components/OrphanPage", async (importOriginal) => {
  const { OrphanPage: RealOrphanPage } = await importOriginal<typeof import("../components/OrphanPage")>();
  return {
    OrphanPage: (props: ComponentProps<typeof OrphanPage>) => {
      pages.orphan = props;
      return <section data-testid="orphans">{native.showRealOrphanPage && <RealOrphanPage {...props} />}</section>;
    },
  };
});
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
import { reviewSessionFor, reviewVerdictFor } from "../lib/rescanRounds";

const demo: InstalledApp = {
  name: "Demo", version: "1", publisher: "Acme", install_location: "C:\\Apps\\Demo",
  uninstall_string: "un.exe", quiet_uninstall_string: "", source: "HKLM64",
  registry_key: "demo", estimated_size_kb: 100, install_date: "", display_icon: "",
};
const other: InstalledApp = { ...demo, name: "Other", registry_key: "other" };
function inventory(...apps: InstalledApp[]): InstalledAppsSnapshot {
  return { apps, completeness: { status: "complete", roots_scanned: 4, failed_roots: [] } };
}
const scan: ScanResult = {
  app_name: demo.name, app_key: appKey(demo),
  items: [{ path: "C:\\Apps\\Demo\\cache", kind: "dir", score: 90,
    confidence: "confirmed", risk: "low", reason: "install", evidence: [] }],
};
const report: FullCleanupReport = {
  app_name: demo.name, app_key: appKey(demo), dry_run: false, backup_dir: "", uninstall_ok: true, uninstall_review: 42,
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
  // The first-run safety card has its own tests; keep the integration suite
  // focused on the features it asserts.
  localStorage.setItem("remova_onboarded_v1", "seen");
  i18n.setLang("zh");
  for (const key of Object.keys(pages) as (keyof typeof pages)[]) pages[key] = undefined;
  native.drag = undefined;
  native.showRealSoftwarePage = false;
  native.showRealOrphanPage = false;
  native.listApps.mockResolvedValue(inventory(demo, other));
  native.analyze.mockResolvedValue(scan);
  native.dryRun.mockResolvedValue(dryReport);
  native.fullCleanup.mockResolvedValue(report);
  native.beginCleanupOperation.mockResolvedValue(51);
  native.cancelCleanup.mockResolvedValue(true);
  native.installMonitorStatus.mockResolvedValue("idle");
  native.discardInstallMonitorRecovery.mockResolvedValue(undefined);
  native.officialUninstall.mockResolvedValue({ ok: true, had_command: true, message: "finished", uninstall_review: 42 });
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
afterEach(() => { cleanup(); toast.clear(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("App orchestration", () => {
  it.each([
    ["zh", "en", "resolve"], ["zh", "en", "reject"],
    ["en", "zh", "resolve"], ["en", "zh", "reject"],
  ] as const)("regenerates the real report after %s to %s and old %s without duplicate manual requests", async (from, to, outcome) => {
    native.showRealSoftwarePage = true;
    const requests: { resolve: (value: string) => void; reject: (error: Error) => void }[] = [];
    native.aiSummarizeReport.mockReset().mockImplementation(() => new Promise<string>((resolve, reject) => {
      requests.push({ resolve, reject });
    }));
    await mount();
    if (i18n.currentLang() !== from) fireEvent.click(screen.getByRole("button", { name: i18n.t().langToggle }));
    act(() => software().setReport(report));
    await nav("more");
    act(() => more().onAiEnabledChange?.(true));
    await nav("software");
    expect(requests).toHaveLength(1);
    expect(screen.getByRole<HTMLButtonElement>("button", { name: i18n.t().aiReportBusy }).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: i18n.t().langToggle }));
    expect(i18n.currentLang()).toBe(to);
    expect(requests).toHaveLength(2);
    await act(async () => {
      if (outcome === "resolve") requests[0]!.resolve("old-language result");
      else requests[0]!.reject(new Error("old-language failure"));
    });
    expect(software().aiReportBusy).toBe(true);
    expect(software().aiReportNote).toBeNull();
    await act(async () => { requests[1]!.resolve("current-language result"); });
    expect(screen.getByText("current-language result")).toBeTruthy();
    const button = screen.getByRole<HTMLButtonElement>("button", { name: i18n.t().aiReportSummary });
    expect(button.disabled).toBe(false);
    const regenerate = software().onRegenerateAiReport;
    act(() => { fireEvent.click(button); regenerate(); });
    expect(requests).toHaveLength(3);
    await act(async () => { requests[2]!.resolve("manual result"); });
    expect(software().aiReportBusy).toBe(false);
    expect(screen.getByText("manual result")).toBeTruthy();
    expect(screen.queryByText("old-language result")).toBeNull();
  });

  it.each(["zh", "en"] as const)("keeps orphan results across navigation without showing another cleanup's details (%s)", async lang => {
    native.showRealOrphanPage = true;
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    const path = "C:\\Apps\\Reused";
    const orphanReport: FullCleanupReport = { ...report, app_name: "Orphans", app_key: undefined,
      uninstall_ok: false, uninstall_review: undefined, deleted: 0, skipped: 1, item_details: [
        { path, kind: "dir", status: "skipped", message: "orphan directory now has an installed owner; scan again" },
      ] };
    native.orphanScan.mockReset().mockResolvedValueOnce({
      items: [{ ...scan.items[0]!, path }], omitted: 0, cleanup_allowed: true, scope_revision: 73,
      completeness: { status: "complete", roots_scanned: 4, failed_roots: [] },
    }).mockResolvedValue({ items: [], omitted: 0, cleanup_allowed: true, scope_revision: 74,
      completeness: { status: "complete", roots_scanned: 4, failed_roots: [] } });
    native.fullCleanup.mockResolvedValueOnce(orphanReport);
    await mount();
    if (i18n.currentLang() !== lang) fireEvent.click(screen.getByRole("button", { name: i18n.t().langToggle }));
    await nav("orphans");
    fireEvent.click(await screen.findByRole("button", { name: `${i18n.t().cleanup} (1)` }));
    await screen.findByText(i18n.t().orphanScanEmpty);
    expect(screen.getByRole("region", { name: i18n.t().orphanCleanupResult })).toBeTruthy();
    await nav("software");
    expect(software().report).toEqual(orphanReport);
    act(() => software().setReport({ ...report, app_name: "Other", item_details: [
      { path: "C:\\Apps\\Other", kind: "dir", status: "deleted", message: "" },
    ] }));
    fireEvent.click(screen.getByRole("button", { name: i18n.t().langToggle }));
    await nav("orphans");
    const result = screen.getByRole("region", { name: i18n.t().orphanCleanupResult });
    fireEvent.click(within(result).getByText(`${i18n.t().reportSkipped} · 1`, { selector: "summary" }));
    expect(within(result).getByText(path)).toBeTruthy();
    expect(within(result).getByText(i18n.t().backendOrphanOwnerChanged)).toBeTruthy();
    expect(within(result).queryByText("C:\\Apps\\Other")).toBeNull();
    expect(screen.queryByRole("checkbox", { name: path })).toBeNull();
    expect(native.fullCleanup).toHaveBeenCalledOnce();
    expect(native.orphanScan).toHaveBeenCalledTimes(3);
  });

  it("shows readable apps during a partial inventory and clears the notice on retry", async () => {
    native.showRealSoftwarePage = true;
    native.listApps.mockResolvedValueOnce({
      apps: [demo],
      completeness: { status: "partial", roots_scanned: 3, failed_roots: ["HKCU"] },
    } satisfies InstalledAppsSnapshot);
    await mount();
    expect(software().apps).toEqual([demo]);
    expect(screen.getByText(i18n.t().appInventoryPartial)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: i18n.t().retry }));
    await waitFor(() => expect(screen.queryByText(i18n.t().appInventoryPartial)).toBeNull());
    expect(software().apps).toEqual([demo, other]);
  });

  it.each(["complete", "partial", "unknown"] as const)("shows orphan completeness from IPC in software checkup (%s)", async (status) => {
    native.showRealSoftwarePage = true;
    const items = scan.items;
    native.orphanScan.mockResolvedValueOnce(status === "unknown" ? items : {
      items,
      omitted: 0,
      completeness: {
        status,
        roots_scanned: 3,
        failed_roots: status === "partial" ? ["inventory:HKCU"] : [],
      },
    });
    await mount();
    act(() => software().setCheckupOpen(true));
    const dialog = within(screen.getByRole("dialog", { name: i18n.t().checkupTitle }));
    fireEvent.click(dialog.getByRole("button", { name: i18n.t().checkupRunOrphan }));

    await waitFor(() => expect(software().checkupOrphanCount).toBe(1));
    const row = dialog.getByText(i18n.t().checkupStepOrphan).parentElement;
    expect(row?.textContent).toContain(status === "complete" ? "✓" : "○");
    expect(row?.textContent).toContain(status === "complete" ? "1" : status === "partial" ? "1+" : "—");
    expect(Boolean(dialog.queryByText(i18n.t().checkupOrphanFound(1)))).toBe(status === "complete");
    if (status === "partial") expect(dialog.getByText("inventory:HKCU")).toBeTruthy();
    if (status === "unknown") expect(dialog.getByText(i18n.t().scanCompletenessUnknown)).toBeTruthy();
  });

  it("reopens the safety card on More instead of only setting hidden state", async () => {
    await mount();
    await nav("more");
    expect(screen.queryByText(i18n.t().onboardingTitle)).toBeNull();
    act(() => more().onReopenOnboarding?.());
    expect(screen.getByText(i18n.t().onboardingTitle)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: i18n.t().onboardingDismiss }));
    expect(screen.queryByText(i18n.t().onboardingTitle)).toBeNull();
  });

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
    await analyzeDemo();
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

  it("rescans from the report without preselecting new findings", async () => {
    saveRescanAfterUninstall(false);
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    await mount();
    await analyzeDemo();
    act(() => software().onCleanup());
    await waitFor(() => expect(software().report).not.toBeNull());
    // Cleanup clears the prior selection; the rescan must not repopulate it.
    expect(software().selectedPaths.size).toBe(0);
    act(() => software().listAnalyze(demo, { afterCleanup: true, uninstallReview: 42 }));
    await waitFor(() => expect(native.analyze).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(software().residualFromUninstall).toBe(true));
    expect(software().selectedPaths.size).toBe(0);
    expect(software().report).not.toBeNull();
  });

  it("discards the dry-run preview on rescan without marking the app uninstalled", async () => {
    saveRescanAfterUninstall(false);
    await mount();
    await analyzeDemo();
    act(() => software().onDryRun());
    await waitFor(() => expect(software().report?.dry_run).toBe(true));
    act(() => software().listAnalyze(demo, { keepReport: true }));
    await waitFor(() => expect(native.analyze).toHaveBeenCalledTimes(2));
    expect(software().residualFromUninstall).toBe(false);
    expect(software().selectedPaths.size).toBe(0);
    expect(software().report).toBeNull();
  });

  it("hides an old or late cleanup preview after the selected action changes", async () => {
    const directory = scan.items[0]!;
    const path = { ...directory, kind: "path" as const };
    native.analyze.mockResolvedValue({ ...scan, items: [directory, path] });
    let finish!: (preview: CleanupReport) => void;
    native.dryRun.mockReturnValueOnce(new Promise<CleanupReport>(resolve => { finish = resolve; }));
    await mount();
    act(() => software().listAnalyze(demo));
    await waitFor(() => expect(software().scan?.items).toHaveLength(2));
    act(() => software().setSelectedPaths(new Set([cleanupItemKey(directory)])));
    act(() => software().onDryRun());
    expect(native.dryRun).toHaveBeenLastCalledWith(demo, [directory], expect.anything());

    act(() => software().setSelectedPaths(new Set([cleanupItemKey(path)])));
    await act(async () => { finish(dryReport); });
    expect(software().report).toBeNull();
    act(() => software().onDryRun());
    await waitFor(() => expect(software().report).toEqual(dryReport));
    expect(native.dryRun).toHaveBeenLastCalledWith(demo, [path], expect.anything());
    act(() => software().setSelectedPaths(new Set([cleanupItemKey(directory)])));
    expect(software().report).toBeNull();
  });

  it("matches full path boundaries in pending and drag entry points", async () => {
    const adjacent = { ...other, install_location: `${demo.install_location}Bar` };
    const nested = { ...other, name: "Nested", install_location: `${demo.install_location}\\Nested` };
    expect(appForPath([demo, adjacent], `${adjacent.install_location}\\file.exe`)).toEqual(adjacent);
    expect(appForPath([demo, nested], `${nested.install_location}\\file.exe`)).toEqual(nested);
    expect(appForPath([demo, { ...other, install_location: demo.install_location }], demo.install_location)).toBeNull();
    expect(appForPath([demo], "C:\\Apps")).toBeNull();
    native.listApps.mockResolvedValue(inventory(demo, adjacent));
    native.takePendingAnalyze.mockResolvedValueOnce(`${demo.install_location}Extra/app.exe`);
    await mount();
    await waitFor(() => expect(native.takePendingAnalyze).toHaveBeenCalled());
    expect(native.analyze).not.toHaveBeenCalled();
    await waitFor(() => expect(native.drag).toBeTruthy());
    act(() => native.drag!({ payload: { type: "drop", paths: ["C:\\Apps"] } }));
    expect(native.analyze).not.toHaveBeenCalled();
    const L = i18n.t();
    const inspector = await screen.findByRole("dialog", { name: L.folderInspectTitle });
    expect(within(inspector).getByRole<HTMLInputElement>("textbox", { name: L.folderInspectPath }).value).toBe("C:\\Apps");
    fireEvent.click(within(inspector).getByRole("button", { name: L.panelClose }));
    act(() => native.drag!({ payload: { type: "drop", paths: [`${adjacent.install_location}/app.exe`] } }));
    await waitFor(() => expect(native.analyze).toHaveBeenCalledWith(adjacent, expect.any(Number)));
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
    fireEvent.click(screen.getByRole("button", { name: i18n.t().themeDark }));
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
      software().setUseOfficial(false);
      software().setEvidence("shared");
      software().setAiRisk("high");
      software().setAiReportNote("old");
      software().setSelectedPaths(new Set([cleanupItemKey(scan.items[0]!)]));
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
      useOfficial: true, residualFromUninstall: false, evidence: null, aiRisk: null,
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

  it("offers cancellation only for the active write and sends its exact operation ID", async () => {
    native.showRealSoftwarePage = true;
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    native.beginCleanupOperation.mockResolvedValueOnce(71).mockResolvedValueOnce(72);
    saveRescanAfterUninstall(false);
    let finishPreview!: (value: CleanupReport) => void;
    let finishWrite!: (value: FullCleanupReport) => void;
    let finishNext!: (value: FullCleanupReport) => void;
    native.dryRun.mockReturnValueOnce(new Promise(resolve => { finishPreview = resolve; }));
    native.fullCleanup.mockReturnValueOnce(new Promise(resolve => { finishWrite = resolve; }))
      .mockReturnValueOnce(new Promise(resolve => { finishNext = resolve; }));

    await mount();
    await analyzeDemo();
    act(() => software().onDryRun());
    expect(screen.getByText(i18n.t().dryRunRunning)).toBeTruthy();
    expect(screen.queryByRole("button", { name: i18n.t().cancelCleanup })).toBeNull();
    await nav("more");
    await nav("software");
    expect(software().dryRunning).toBe(true);
    expect(software().onCancelCleanup).toBeUndefined();
    expect(native.beginCleanupOperation).not.toHaveBeenCalled();
    expect(native.cancelCleanup).not.toHaveBeenCalled();

    await act(async () => finishPreview(dryReport));
    act(() => software().onCleanup());
    const button = await screen.findByRole("button", { name: i18n.t().cancelCleanup });
    expect(software().dryRunning).toBe(false);
    const oldCancel = software().onCancelCleanup!;
    fireEvent.click(button);
    expect(native.cancelCleanup).toHaveBeenCalledExactlyOnceWith(71);
    await act(async () => finishWrite(report));
    await waitFor(() => expect(software().busy).toBe(false));
    await analyzeDemo();
    act(() => software().onCleanup());
    await waitFor(() => expect(native.fullCleanup).toHaveBeenLastCalledWith(demo, scan.items, expect.anything(), 72));
    act(oldCancel);
    expect(native.cancelCleanup).toHaveBeenCalledTimes(1);
    fireEvent.click(await screen.findByRole("button", { name: i18n.t().cancelCleanup }));
    expect(native.cancelCleanup).toHaveBeenLastCalledWith(72);
    await act(async () => finishNext(report));
  });

  it("keeps a temporary final verification failure retryable without repeating the vendor uninstall", async () => {
    native.showRealSoftwarePage = true;
    saveRescanAfterUninstall(false);
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    native.fullCleanup.mockResolvedValueOnce({ ...report, aborted: true, deleted: 0,
      uninstall_message: "uninstall review could not verify installation" })
      .mockResolvedValueOnce({ ...report, uninstall_ok: false, uninstall_message: "skipped" });
    await mount();
    await analyzeDemo();
    act(() => software().onCleanup());
    await waitFor(() => expect(software().uninstallReviews[0]).toMatchObject({ receipt: 42, rescan: { phase: "unverified" } }));
    expect(software().scan).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: i18n.t().reportRescanApp }));
    await waitFor(() => expect(software().scanning).toBe(false));
    await waitFor(() => expect(software().scan).not.toBeNull());
    act(() => software().setSelectedPaths(new Set(scan.items.map(cleanupItemKey))));
    act(() => software().onCleanup());
    await waitFor(() => expect(native.fullCleanup).toHaveBeenCalledTimes(2));
    expect(native.fullCleanup.mock.calls.map(call => call[2].skip_official_uninstall)).toEqual([false, true]);
    expect(native.fullCleanup.mock.calls[1]?.[2].uninstall_review).toBe(42);
  });

  it("only skips official uninstall explicitly and resets the choice on a new scan", async () => {
    saveRescanAfterUninstall(false);
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    await mount();
    await analyzeDemo();
    act(() => software().setUseOfficial(false));
    act(() => software().onCleanup());
    await waitFor(() => expect(native.fullCleanup).toHaveBeenCalled());
    expect(native.fullCleanup).toHaveBeenCalledWith(expect.anything(), expect.anything(),
      expect.objectContaining({ skip_official_uninstall: true }), 51);
    await waitFor(() => expect(software().busy).toBe(false));
    await analyzeDemo();
    expect(software().useOfficial).toBe(true);
  });

  it.each([true, false])("post-cleanup reanalysis follows preference %s", async (enabled) => {
    saveRescanAfterUninstall(enabled);
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    await mount();
    await analyzeDemo();
    expect(software().useOfficial).toBe(true);
    act(() => software().onCleanup());
    await waitFor(() => expect(native.fullCleanup).toHaveBeenCalled());
    expect(native.fullCleanup).toHaveBeenCalledWith(expect.anything(), expect.anything(),
      expect.objectContaining({ skip_official_uninstall: false }), 51);
    await waitFor(() => expect(native.analyze).toHaveBeenCalledTimes(enabled ? 2 : 1));
    await waitFor(() => expect(software().uninstallReviews[0]).toEqual({ app: demo, receipt: 42,
      rescan: enabled ? { phase: "complete", count: scan.items.length } : { phase: "not_started" } }));
    expect(software().residualFromUninstall).toBe(enabled);
    expect(software().report).toEqual(report);
    expect(software().selectedPaths.size).toBe(0);
  });

  it.each([true, false])("exports review rounds from the real report rescan button (automatic=%s)", async (automatic) => {
    native.showRealSoftwarePage = true;
    saveRescanAfterUninstall(automatic);
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    const blobs: Blob[] = [];
    vi.stubGlobal("crypto", { subtle: { digest: async () => new ArrayBuffer(32) } });
    vi.stubGlobal("URL", class extends URL {
      static createObjectURL(blob: Blob) { blobs.push(blob); return "blob:review-audit"; }
      static revokeObjectURL() {}
    });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});

    await mount();
    await analyzeDemo();
    act(() => software().onCleanup());
    await waitFor(() => expect(software()).toMatchObject({ report, scanning: false, dryRunning: false }));
    expect(native.analyze).toHaveBeenCalledTimes(automatic ? 2 : 1);

    const rescan = async (count: number) => {
      fireEvent.click(screen.getByRole("button", { name: i18n.t().reportRescanApp }));
      await waitFor(() => {
        expect(native.analyze).toHaveBeenCalledTimes(count);
        expect(software().scanning).toBe(false);
      });
    };
    if (!automatic) await rescan(2);
    await rescan(3);

    const readExport = async (count: number) => {
      fireEvent.click(screen.getByRole("button", { name: i18n.t().auditExportJson }));
      await waitFor(() => expect(blobs).toHaveLength(count));
      const exported = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => {
          if (typeof reader.result === "string") resolve(reader.result);
          else reject(new Error("Expected a text audit report"));
        };
        reader.onerror = reject;
        reader.readAsText(blobs[count - 1]!);
      });
      return JSON.parse(exported);
    };
    expect(await readExport(1)).toMatchObject({ review_rounds: 2, review_converged: true });
    native.analyze.mockRejectedValueOnce("rescan denied");
    await rescan(4);
    expect(await readExport(2)).toMatchObject({
      review_rounds: 3,
      review_converged: null,
      review_unavailable_reason: "incomplete",
    });
    if (!automatic) {
      const previousSession = reviewSessionFor(report);
      act(() => software().listForceClean(demo));
      await waitFor(() => {
        expect(native.fullCleanup).toHaveBeenCalledTimes(2);
        expect(reviewSessionFor(report)).not.toBe(previousSession);
      });
      expect(await readExport(3)).toMatchObject({
        review_rounds: 0,
        review_converged: null,
        review_unavailable_reason: "insufficient",
      });
    }
  });

  it.each(["empty", "remaining", "failed", "cancelled"] as const)("preserves cleanup results through a slow %s rescan without selecting stale targets", async (state) => {
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    await mount();
    await analyzeDemo();
    let finish!: (value: ScanResult) => void;
    let fail!: (reason: string) => void;
    native.analyze.mockReturnValueOnce(new Promise<ScanResult>((resolve, reject) => { finish = resolve; fail = reject; }));
    act(() => software().onCleanup());
    await waitFor(() => expect(native.analyze).toHaveBeenCalledTimes(2));
    expect(software()).toMatchObject({ report, scan: null, scanning: true });
    expect(software().selectedPaths.size).toBe(0);
    if (state === "cancelled") fireEvent.click(screen.getByRole("button", { name: i18n.t().cancelScan }));
    await act(async () => {
      if (state === "failed") fail("rescan denied");
      else finish({ ...scan, items: state === "empty" ? [] : scan.items });
    });
    await waitFor(() => expect(software().scanning).toBe(false));
    expect(software().report).toEqual(report);
    expect(software().selectedPaths.size).toBe(0);
    if (state === "failed" || state === "cancelled") {
      expect(software().scan).toBeNull();
      expect(software().uninstallReviews[0]).toEqual({ app: demo, receipt: 42, rescan: { phase: state } });
      expect(reviewVerdictFor(reviewSessionFor(report)).state).toBe("incomplete_round");
    }
    else expect(software().scan?.items.length).toBe(state === "empty" ? 0 : scan.items.length);
    if (state === "remaining") {
      act(() => software().setSelectedPaths(new Set([cleanupItemKey(scan.items[0]!)])));
      act(() => software().onCleanup());
      await waitFor(() => expect(native.fullCleanup).toHaveBeenCalledTimes(2));
      expect(native.fullCleanup).toHaveBeenLastCalledWith(expect.anything(), expect.anything(),
        expect.objectContaining({ skip_official_uninstall: true }), 51);
    }
  });

  it.each([true, false])("keeps official-only completion distinct from pending residual cleanup (scan=%s)", async checked => {
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked });
    await mount();
    act(() => software().listStartUninstall(demo));
    await waitFor(() => expect(native.officialUninstall).toHaveBeenCalledWith(demo));
    await waitFor(() => expect(software().uninstallReviews[0]?.app).toEqual(demo));
    if (checked) await waitFor(() => expect(software().scan).toEqual(scan));
    else expect(software().scan).toBeNull();
    expect(software().residualFromUninstall).toBe(checked);
    expect(software().report).toBeNull();
    expect(native.fullCleanup).not.toHaveBeenCalled();
  });

  it("can retry a cancelled leftover scan and still runs the official uninstaller for a same-name variant", async () => {
    const sibling = { ...demo, source: "HKLM32" as const };
    native.listApps.mockResolvedValueOnce(inventory(demo, sibling)).mockResolvedValue(inventory(sibling));
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: true });
    native.analyze.mockRejectedValueOnce(new Error("scan:cancelled"));
    await mount();
    act(() => software().listStartUninstall(demo));
    await waitFor(() => expect(software().uninstallReviews[0]).toEqual({ app: demo, receipt: 42, rescan: { phase: "cancelled" } }));
    await waitFor(() => expect(software().selected).toBeNull());
    act(() => software().listAnalyze(demo, { fromUninstall: true, uninstallReview: 42 }));
    await waitFor(() => expect(software().scan).toEqual(scan));
    expect(software().residualFromUninstall).toBe(true);
    expect(native.officialUninstall).toHaveBeenCalledTimes(1);

    act(() => software().selectApp(sibling));
    expect(software().residualFromUninstall).toBe(false);
    native.analyze.mockResolvedValueOnce({ ...scan, app_key: appKey(sibling) });
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    act(() => software().listAnalyze(sibling));
    await waitFor(() => expect(software().scan?.app_key).toBe(appKey(sibling)));
    act(() => software().onCleanup());
    await waitFor(() => expect(native.fullCleanup).toHaveBeenCalledWith(sibling, scan.items,
      expect.objectContaining({ skip_official_uninstall: false }), 51));
  });

  it("keeps two uninstall reviews available after analyzing a third app and reopening Software", async () => {
    const retained = { ...demo, name: "Retained", registry_key: "retained", install_location: "C:\\Apps\\Retained" };
    const second = { ...other, name: demo.name, install_location: "C:\\Apps\\Second" };
    native.showRealSoftwarePage = true;
    native.listApps.mockResolvedValueOnce(inventory(demo, second, retained))
      .mockResolvedValueOnce(inventory(second, retained)).mockResolvedValue(inventory(retained));
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    native.officialUninstall.mockResolvedValueOnce({ ok: true, had_command: true, message: "finished", uninstall_review: 42 })
      .mockResolvedValueOnce({ ok: true, had_command: true, message: "finished", uninstall_review: 43 });
    native.analyze.mockResolvedValueOnce({ ...scan, app_name: retained.name, app_key: appKey(retained) })
      .mockResolvedValueOnce(scan);

    await mount();
    for (const app of [demo, second]) {
      act(() => software().listStartUninstall(app));
      await waitFor(() => expect(software().uninstallReviews[0]?.app).toEqual(app));
      await waitFor(() => expect(software().uninstallingKey).toBeNull());
    }
    act(() => software().listAnalyze(retained));
    await waitFor(() => expect(software().scan?.app_key).toBe(appKey(retained)));
    act(() => software().onBack());
    expect(software().scan).toBeNull();
    await nav("more");
    await nav("software");
    expect(software().uninstallReviews.map(review => review.receipt)).toEqual([43, 42]);
    const queue = within(screen.getByRole("region", { name: i18n.t().uninstallReviewList }));
    const buttons = queue.getAllByRole("button", { name: new RegExp(i18n.t().reportRescanApp) });
    expect(buttons).toHaveLength(2);
    fireEvent.click(buttons[1]!);
    await waitFor(() => expect(native.analyze).toHaveBeenLastCalledWith(demo, expect.anything(), 42));
    await waitFor(() => expect(software().scanReviewReceipt).toBe(42));
    expect(software().uninstallReviews[0]?.receipt).toBe(43);
  });

  it("retires the retry after a reinstall refusal and keeps a new ordinary scan independent", async () => {
    native.showRealSoftwarePage = true;
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    native.analyze.mockRejectedValueOnce("uninstall review found an installed application").mockResolvedValueOnce(scan);
    await mount();
    act(() => software().listStartUninstall(demo));
    await waitFor(() => expect(software().uninstallReviews[0]?.receipt).toBe(42));
    await waitFor(() => expect(software().uninstallingKey).toBeNull());
    const queue = within(screen.getByRole("region", { name: i18n.t().uninstallReviewList }));
    fireEvent.click(queue.getByRole("button", { name: new RegExp(i18n.t().reportRescanApp) }));
    await waitFor(() => expect(software().uninstallReviews[0]?.rescan.phase).toBe("installed"));
    expect(queue.queryByRole("button")).toBeNull();
    act(() => software().listAnalyze(demo));
    await waitFor(() => expect(software().scan).toEqual(scan));
    expect(native.analyze).toHaveBeenLastCalledWith(demo, expect.anything());
    expect(software().scanReviewReceipt).toBeUndefined();
  });

  it.each([
    ["deep", "zh", true], ["official", "en", false],
    ["deep", "en", false], ["official", "zh", true],
  ] as const)("keeps the %s 3010 notice visible in %s (verified=%s) after the toast closes", async (entry, language, verified) => {
    i18n.setLang(language);
    const L = i18n.t();
    native.showRealSoftwarePage = true;
    native.listApps.mockResolvedValueOnce(inventory(demo, other))
      .mockResolvedValue(verified ? inventory(other) : inventory(demo, other));
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    native.requestConfirm.mockResolvedValue(true);
    saveRescanAfterUninstall(false);
    native.officialUninstall.mockResolvedValueOnce({
      ok: verified, had_command: true,
      message: verified ? "uninstaller finished (reboot required): msiexec.exe" : "uninstall requires reboot before verification",
      ...(verified && { uninstall_review: 42 }),
    });
    await mount();
    if (entry === "deep") act(() => software().listStartUninstall(demo));
    else {
      act(() => software().selectApp(demo));
      await screen.findByTestId("detail");
      act(() => detail().onOfficialOnly?.(demo));
    }
    await waitFor(() => expect(software().uninstallingKey).toBeNull());
    const notice = verified ? L.backendRebootRequired : L.backendUninstallRebootBeforeVerify;
    expect(await screen.findAllByText(notice)).not.toHaveLength(0);
    act(() => toast.clear());
    await nav("more");
    await nav("software");
    const queue = within(screen.getByRole("region", { name: L.uninstallReviewList }));
    expect(queue.getByText(notice)).toBeTruthy();
    expect(queue.queryByRole("button") !== null).toBe(verified);
    expect(native.analyze).not.toHaveBeenCalled();
  });

  it("blocks native drops and a new uninstall until the automatic rescan and refresh settle", async () => {
    const dropped = { ...other, install_location: "C:\\Apps\\Dropped" };
    const third = { ...demo, name: "Third", registry_key: "third", install_location: "C:\\Apps\\Third" };
    let finishScan!: (value: ScanResult) => void;
    let finishRefresh!: (value: InstalledAppsSnapshot) => void;
    native.listApps.mockResolvedValueOnce(inventory(demo, dropped, third))
      .mockReturnValueOnce(new Promise(resolve => { finishRefresh = resolve; }));
    native.analyze.mockReturnValueOnce(new Promise(resolve => { finishScan = resolve; }))
      .mockResolvedValueOnce({ ...scan, app_name: dropped.name, app_key: appKey(dropped) });
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: true });
    await mount();
    await waitFor(() => expect(native.drag).toBeTruthy());
    act(() => software().listStartUninstall(demo));
    await waitFor(() => expect(software().scanning).toBe(true));
    const drop = () => native.drag!({ payload: { type: "drop", paths: [`${dropped.install_location}\\app.exe`] } });
    act(() => { drop(); software().listStartUninstall(third); });
    expect(software().selected).toEqual(demo);
    expect(native.analyze).toHaveBeenCalledTimes(1);
    expect(native.requestConfirmEx).toHaveBeenCalledTimes(1);
    await act(async () => finishScan(scan));
    await waitFor(() => expect(native.listApps).toHaveBeenCalledTimes(2));
    act(() => { drop(); software().listStartUninstall(third); });
    expect(native.analyze).toHaveBeenCalledTimes(1);
    expect(native.officialUninstall).toHaveBeenCalledTimes(1);
    await act(async () => finishRefresh(inventory(dropped, third)));
    await waitFor(() => expect(software().uninstallingKey).toBeNull());
    act(drop);
    await waitFor(() => expect(native.analyze).toHaveBeenLastCalledWith(dropped, expect.anything()));
    expect(software().uninstallReviews[0]?.receipt).toBe(42);
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
    act(() => detail().onOfficialOnly?.(demo));
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

  it("preserves monitor changes when an in-flight app scan owns the foreground", async () => {
    await mount();
    await nav("more");
    act(() => more().onToggleMonitor());
    await waitFor(() => expect(more().monitoring).toBe(true));
    act(() => more().onToggleMonitor());
    await waitFor(() => expect(more().monitorDiff).toBeTruthy());
    const diff = more().monitorDiff;
    let finish!: (value: ScanResult) => void;
    native.analyze.mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
    await nav("software");
    act(() => software().listAnalyze(demo));
    await waitFor(() => expect(software().scanning).toBe(true));
    await nav("more");
    act(() => more().onMonitorToCleanup());
    expect(more().monitorDiff).toBe(diff);
    await act(async () => finish(scan));
    await nav("software");
    expect(software().selected?.source).toBe(demo.source);
    expect(software().scan).toEqual(scan);
  });

  it("retires the old scan when the uninstaller succeeded but leftover cleanup was cancelled", async () => {
    await mount();
    await analyzeDemo();
    native.requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
    native.fullCleanup.mockResolvedValueOnce({ ...report, aborted: true, deleted: 0,
      uninstall_ok: true, uninstall_review: 42, uninstall_message: "cleanup cancelled" });
    await act(async () => software().onCleanup());
    await waitFor(() => expect(software().scan).toBeNull());
    expect(software().selectedPaths.size).toBe(0);
    expect(software().uninstallReviews.some(review => review.receipt === 42)).toBe(true);
    await act(async () => software().onCleanup());
    expect(native.fullCleanup).toHaveBeenCalledTimes(1);
  });

  it("recovers the previous tracking result without offering cleanup", async () => {
    native.installMonitorStatus.mockResolvedValue("recovered");
    native.endInstallMonitor.mockResolvedValue({ diff: {
      added_files: ["C:\\Apps\\fixture.exe"], added_reg_values: [], recovered_read_only: true,
    }, items: [] });
    await mount();
    await nav("more");
    expect(more().monitorRecovery).toBe("recovered");
    const panel = render(<MonitorPanel diff={null} recovery={more().monitorRecovery} onToggle={more().onToggleMonitor}
      onToCleanup={more().onMonitorToCleanup} onDismiss={() => {}} />);
    fireEvent.click(panel.getByRole("button", { name: i18n.t().monitorReviewRecovery }));
    await waitFor(() => expect(more().monitorDiff?.recovered_read_only).toBe(true));
    expect(native.beginInstallMonitor).not.toHaveBeenCalled();
    panel.rerender(<MonitorPanel diff={more().monitorDiff} onToCleanup={more().onMonitorToCleanup} onDismiss={() => {}} />);
    expect(panel.getByText(i18n.t().monitorRecoveryReadOnly)).toBeTruthy();
    expect(panel.queryByRole("button", { name: i18n.t().monitorToCleanup })).toBeNull();
  });

  it("requires explicit discard before starting when the previous snapshot cannot open", async () => {
    native.installMonitorStatus.mockResolvedValue("unavailable");
    await mount();
    await nav("more");
    expect(more().monitorRecovery).toBe("unavailable");
    await act(async () => more().onToggleMonitor());
    expect(native.beginInstallMonitor).not.toHaveBeenCalled();
    await act(async () => more().onDiscardMonitorRecovery?.());
    expect(native.discardInstallMonitorRecovery).not.toHaveBeenCalled();
    native.requestConfirm.mockResolvedValue(true);
    await act(async () => more().onDiscardMonitorRecovery?.());
    await waitFor(() => expect(more().monitorRecovery).toBeNull());
    expect(native.discardInstallMonitorRecovery).toHaveBeenCalledTimes(1);
    act(() => more().onToggleMonitor());
    await waitFor(() => expect(more().monitoring).toBe(true));
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

  it.each(["denied", "no monitor snapshot; start first"])("handles a failed final snapshot without stale cleanup: %s", async message => {
    await mount();
    await nav("more");
    act(() => more().onToggleMonitor());
    await waitFor(() => expect(more().monitoring).toBe(true));
    native.endInstallMonitor.mockRejectedValueOnce(new Error(message));
    act(() => more().onToggleMonitor());
    const active = message === "denied";
    expect(await within(screen.getByRole("main")).findByRole("status"))
      .toHaveProperty("textContent", i18n.t().monitorScanFailed(active));
    expect(more().monitoring).toBe(active);
    expect(more().monitorDiff).toBeNull();
    expect(more().monitorSnapshotBusy).toBe(false);
    expect(native.fullCleanup).not.toHaveBeenCalled();
    act(() => more().onToggleMonitor());
    if (active) await waitFor(() => expect(more().monitorDiff).toBeTruthy());
    else await waitFor(() => expect(more().monitoring).toBe(true));
  });

  it("hands pending analyze and native drag-drop into the App scan flow", async () => {
    native.listApps.mockResolvedValue(inventory(demo));
    native.takePendingAnalyze.mockResolvedValueOnce("C:\\Apps\\Demo\\app.exe");
    await mount();
    await waitFor(() => expect(software().scan).toEqual(scan));
    expect(native.analyze).toHaveBeenCalledWith(demo, 1);
    await waitFor(() => expect(native.drag).toBeTruthy());
    act(() => native.drag!({ payload: { type: "drop", paths: ["C:\\Apps\\Demo"] } }));
    await waitFor(() => expect(native.analyze).toHaveBeenCalledTimes(2));
  });

  // performance fix: the grouped setter bags handed to useCleanupHandlers /
  // useAnalyzeFlow / useAppChrome were object literals, so `doIgnoreApp` got a new
  // identity on every App render → `listIgnoreApp` → `softwareProps` → `memo`
  // never held and every visible AppRow re-rendered. Assert the callback
  // identities directly: the page's props object is rebuilt on every App render
  // (it is a spread), so only the individual identities prove the bail-out.
  it("keeps the software page's action identities stable across unrelated chrome state", async () => {
    native.listApps.mockResolvedValue(inventory(demo, other));
    await mount();
    // Let the boot chain settle; its promises can land after `mount()` returns.
    let quiet = 0;
    for (let i = 0; i < 60 && quiet < 4; i++) {
      const settled = software().listIgnoreApp;
      await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
      quiet = software().listIgnoreApp === settled ? quiet + 1 : 0;
    }
    const before = software();
    const keys = ["listIgnoreApp", "listIgnorePublisher", "listAnalyze", "listForceClean",
      "listStartUninstall", "onCleanup", "onDryRun", "toggleMulti"] as const;
    const beforeIds = Object.fromEntries(keys.map(k => [k, before[k]]));
    fireEvent.click(screen.getByRole("button", { name: i18n.t().themeDark }));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
    for (const k of keys) {
      expect(software()[k], `theme switch changed software().${k}`).toBe(beforeIds[k]);
    }
    // Control: the language switch rebuilds every copy object, so the page *must*
    // get new identities. Without this the assertions above could pass vacuously.
    fireEvent.click(screen.getByRole("button", { name: i18n.t().langToggle }));
    await waitFor(() => expect(software().listIgnoreApp).not.toBe(beforeIds.listIgnoreApp));
  });
});

it("keeps the newer inventory when an earlier preview refresh completes late", async () => {
  const partial = { ...inventory(demo, other), completeness: { status: "partial", roots_scanned: 3, failed_roots: ["HKCU"] } };
  native.listApps.mockResolvedValueOnce(partial);
  await mount();
  await analyzeDemo();
  let settle!: (v: InstalledAppsSnapshot) => void;
  native.listApps.mockImplementationOnce(() => new Promise(resolve => { settle = resolve; }));
  act(() => software().onBack());
  await waitFor(() => expect(software().scan).toBeNull());
  expect(software().inventoryIncomplete).toBe(true);
  native.listApps.mockResolvedValueOnce(inventory(other));
  await act(async () => { await software().refreshInventory!(); });
  expect(software().apps).toEqual([other]);
  expect(software().inventoryIncomplete).toBe(false);
  await act(async () => { settle(partial as InstalledAppsSnapshot); });
  expect(software().apps).toEqual([other]);
  expect(software().inventoryIncomplete).toBe(false);
});

it("drains forwarded analyze requests once without refreshing the inventory", async () => {
  const installed = { ...other, install_location: "C:\\Apps\\Other" };
  native.listApps.mockResolvedValue(inventory(demo, installed));
  await mount();
  await waitFor(() => expect(pendingAnalysisEvent.listener).toBeDefined());
  native.takePendingAnalyze
    .mockResolvedValueOnce(demo.install_location)
    .mockResolvedValueOnce(installed.install_location);
  act(() => {
    pendingAnalysisEvent.listener?.();
    pendingAnalysisEvent.listener?.();
  });
  await waitFor(() => expect(native.analyze).toHaveBeenCalledTimes(2));
  expect(native.analyze.mock.calls.map(([app]) => app.name)).toEqual([demo.name, other.name]);
});
