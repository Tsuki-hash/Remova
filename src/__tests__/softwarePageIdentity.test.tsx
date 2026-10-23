// @vitest-environment jsdom
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import type { FullCleanupReport, InstalledApp } from "../types";

// The integration suite mocks SoftwarePage wholesale, so its own identity
// resolution (and the module-level report cache that survives navigating away)
// was never executed. This suite renders the real component.
vi.mock("../lib/api", () => ({
  api: {
    verifyCleanup: vi.fn(async () => []),
    listCleanupHistory: vi.fn(async () => []),
    aiExplain: vi.fn(),
    backupSessions: vi.fn(async () => []),
  },
}));
vi.mock("../components/DeepRegistryCheck", () => ({
  DeepRegistryCheck: () => null,
}));

import { SoftwarePage, clearReportAppCache } from "../components/SoftwarePage";
import { appKey } from "../lib/appKey";

const demo: InstalledApp = {
  name: "Demo",
  version: "1",
  publisher: "Acme",
  install_location: "C:\\Apps\\Demo",
  uninstall_string: "un.exe",
  quiet_uninstall_string: "",
  source: "HKLM64",
  registry_key: "demo",
  estimated_size_kb: 100,
  install_date: "",
  display_icon: "",
};

const report: FullCleanupReport = {
  app_name: "Demo",
  app_key: appKey(demo),
  dry_run: false,
  backup_dir: "",
  uninstall_ok: true,
  uninstall_message: "ok",
  deleted: 1,
  failed: 1,
  skipped: 0,
  aborted: false,
  restore_point_ok: true,
  restore_point_msg: "",
  errors: [],
  item_details: [
    { path: "C:\\Apps\\Demo\\locked.exe", kind: "file", status: "failed", message: "locked" },
  ],
};

function props(over: Partial<ComponentProps<typeof SoftwarePage>> = {}) {
  return {
    apps: [demo],
    filtered: [demo],
    loading: false,
    q: "",
    category: "all" as const,
    sortCol: "name" as const,
    sortDesc: false,
    selected: demo,
    multi: new Set<string>(),
    uninstallingKey: null,
    formatAppSize: () => "100 KB",
    sizeOf: () => 100,
    sortBy: vi.fn(),
    selectApp: vi.fn(),
    setSelected: vi.fn(),
    toggleMulti: vi.fn(),
    listStartUninstall: vi.fn(),
    listAnalyze: vi.fn(),
    listForceClean: vi.fn(),
    listIgnoreApp: vi.fn(),
    listIgnorePublisher: vi.fn(),
    appKey,
    setMulti: vi.fn(),
    setScan: vi.fn(),
    scanning: false,
    scan: null,
    useOfficial: true,
    setUseOfficial: vi.fn(),
    busy: false,
    onCleanup: vi.fn(),
    onDryRun: vi.fn(),
    dryRunning: false,
    onBack: vi.fn(),
    onAiExplain: vi.fn(),
    aiBusy: false,
    aiEnabled: false,
    aiRisk: null,
    setAiRisk: vi.fn(),
    aiSummaryNote: null,
    aiReportBusy: false,
    aiReportNote: null,
    onRegenerateAiReport: vi.fn(),
    ignoreSuggestions: [],
    onIgnoreApplied: vi.fn(),
    onDismissIgnore: vi.fn(),
    residualFromUninstall: false,
    uninstallStage: "idle" as const,
    report,
    setReport: vi.fn(),
    verifyRows: null,
    checkupOpen: false,
    checkup: null,
    checkupOrphanBusy: false,
    checkupScanStatus: null,
    checkupOrphanCount: 0,
    checkupOrphanScan: vi.fn(),
    onGoOrphans: vi.fn(),
    setCheckupOpen: vi.fn(),
    error: null,
    setError: vi.fn(),
    selectedPaths: new Set<string>(),
    setSelectedPaths: vi.fn(),
    onOpenSettings: vi.fn(),
    onCopilotApplyFilter: vi.fn(),
    onCopilotBatch: vi.fn(),
    kindFilter: null,
    setKindFilter: vi.fn(),
    riskFilter: null,
    setRiskFilter: vi.fn(),
    detailPanel: null,
    ...over,
  } as unknown as ComponentProps<typeof SoftwarePage>;
}

describe("SoftwarePage report identity", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // The report cache is module-level; without a reset a case inherits the
    // previous case's identities and the suite becomes order-dependent.
    clearReportAppCache();
  });

  // RTL auto-cleanup is not enabled for this suite, so a previous render would
  // otherwise stay mounted and every query would match twice.
  afterEach(() => {
    cleanup();
  });

  it("offers rescan for a report whose app is still listed", () => {
    render(<SoftwarePage {...props()} />);
    expect(screen.getAllByText("重新扫描该应用").length).toBeGreaterThan(0);
  });

  it("keeps rescan available after the app leaves the list and the page remounts", () => {
    // First mount: the app is still present, so the identity is cached.
    const { unmount } = render(<SoftwarePage {...props()} />);
    expect(screen.getAllByText("重新扫描该应用").length).toBeGreaterThan(0);
    unmount();

    // The uninstall removed the app from the refreshed list, and the reducer
    // cleared `selected`. A page-local cache would be empty after this
    // remount, dropping rescan/lock actions for a report that is still shown.
    const listAnalyze = vi.fn();
    render(
      <SoftwarePage
        {...props({ apps: [], filtered: [], selected: null, listAnalyze })}
      />,
    );
    expect(screen.getAllByText("重新扫描该应用").length).toBeGreaterThan(0);
  });

  it("does not attribute one report's identity to a different app", () => {
    const other: InstalledApp = { ...demo, name: "Other", registry_key: "other" };
    const foreignReport = { ...report, app_name: "Other", app_key: appKey(other) };
    render(
      <SoftwarePage
        {...props({ apps: [], filtered: [], selected: null, report: foreignReport })}
      />,
    );
    // No cached identity exists for this report and the app is gone, so the
    // rescan action must not be offered from another report's cached identity.
    expect(screen.queryByText("重新扫描该应用")).toBeNull();
  });

  it("wires the real cleanup action for a selected scan row", () => {
    const item = {
      path: "C:\\Apps\\Demo\\leftover.bin",
      kind: "file" as const,
      score: 90,
      confidence: "confirmed" as const,
      risk: "low" as const,
      reason: "leftover",
      evidence: [],
      shared: false,
      service_bound: false,
      user_data: false,
      user_library: false,
      size_kb: 1,
      size_capped: false,
      bucket: null,
    };
    const scan = {
      app_name: "Demo",
      app_key: appKey(demo),
      items: [item],
      degraded: [],
    };
    const onCleanup = vi.fn();
    const { container } = render(
      <SoftwarePage
        {...props({
          scan,
          selectedPaths: new Set([item.path]),
          onCleanup,
        })}
      />,
    );
    const button = container.querySelector(".scan-actions-cleanup");
    expect(button).toBeTruthy();
    (button as HTMLButtonElement).click();
    expect(onCleanup).toHaveBeenCalledTimes(1);
  });

  it("offers a cancel action while a cleanup is in progress", () => {
    const item = {
      path: "C:\\Apps\\Demo\\leftover.bin",
      kind: "file" as const,
      score: 90,
      confidence: "confirmed" as const,
      risk: "low" as const,
      reason: "leftover",
      evidence: [],
      shared: false,
      service_bound: false,
      user_data: false,
      user_library: false,
      size_kb: 1,
      size_capped: false,
      bucket: null,
    };
    const scan = {
      app_name: "Demo",
      app_key: appKey(demo),
      items: [item],
      degraded: [],
    };
    const onCancelCleanup = vi.fn();
    const { container } = render(
      <SoftwarePage
        {...props({
          scan,
          selectedPaths: new Set([item.path]),
          dryRunning: true,
          onCancelCleanup,
        })}
      />,
    );
    const button = container.querySelector(".scan-actions-cancel");
    expect(button).toBeTruthy();
    (button as HTMLButtonElement).click();
    expect(onCancelCleanup).toHaveBeenCalledTimes(1);
  });

  it("dismisses the error banner through its real button", () => {
    const setError = vi.fn();
    render(<SoftwarePage {...props({ error: "locked file", setError })} />);
    screen.getByRole("button", { name: "知道了" }).click();
    expect(setError).toHaveBeenCalledWith(null);
  });

  it("dismisses the AI risk banner through its real button", () => {
    const setAiRisk = vi.fn();
    render(
      <SoftwarePage
        {...props({
          aiRisk: "vendor cache",
          scan: { app_name: "Demo", app_key: appKey(demo), items: [], degraded: [] },
          setAiRisk,
        })}
      />,
    );
    const banner = screen.getByText("vendor cache").closest("div")!;
    (banner.querySelector("button") as HTMLButtonElement).click();
    expect(setAiRisk).toHaveBeenCalledWith(null);
  });

  it("renders the post-uninstall residual notice for scanning, empty and unknown scans", () => {
    const { rerender } = render(
      <SoftwarePage {...props({ residualFromUninstall: true, report: null, scanning: true })} />,
    );
    expect(screen.getAllByText(/^分析中/).length).toBeGreaterThan(0);
    rerender(
      <SoftwarePage
        {...props({
          residualFromUninstall: true,
          report: null,
          scan: { app_name: "Demo", app_key: appKey(demo), items: [], degraded: [] },
        })}
      />,
    );
    expect(screen.getByText(/本次复扫未发现关联残留/)).toBeTruthy();
    rerender(<SoftwarePage {...props({ residualFromUninstall: true, report: null, scan: null })} />);
    expect(screen.getByText(/剩余关联项尚未核对/)).toBeTruthy();
  });

  it("expires a cached report identity after 24 hours", () => {
    vi.useFakeTimers();
    try {
      const app = { ...demo, name: "TtlApp", registry_key: "ttl-app" };
      const rep = { ...report, app_name: app.name, app_key: appKey(app) };
      vi.setSystemTime(new Date("2026-10-23T00:00:00Z"));
      const first = render(
        <SoftwarePage {...props({ apps: [app], filtered: [app], selected: null, report: rep })} />,
      );
      first.unmount();
      vi.setSystemTime(new Date("2026-10-24T01:00:00Z"));
      render(<SoftwarePage {...props({ apps: [], filtered: [], selected: null, report: rep })} />);
      expect(screen.queryByText("重新扫描该应用")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("evicts the oldest cached report identity after 64 reports", () => {
    const first = { ...demo, name: "CacheFirst", registry_key: "cache-first" };
    const firstReport = { ...report, app_name: first.name, app_key: appKey(first) };
    const mounted = render(
      <SoftwarePage {...props({ apps: [first], filtered: [first], selected: null, report: firstReport })} />,
    );
    mounted.unmount();
    for (let i = 0; i < 65; i++) {
      const app = { ...demo, name: `Cache${i}`, registry_key: `cache-${i}` };
      const rep = { ...report, app_name: app.name, app_key: appKey(app) };
      const view = render(
        <SoftwarePage {...props({ apps: [app], filtered: [app], selected: null, report: rep })} />,
      );
      view.unmount();
    }
    render(<SoftwarePage {...props({ apps: [], filtered: [], selected: null, report: firstReport })} />);
    expect(screen.queryByText("重新扫描该应用")).toBeNull();
  });
});
