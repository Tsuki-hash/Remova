// @vitest-environment jsdom
// REV-QA-02: software controller mapping — copilot scope handoff, busy
// composite, orphan/checkup nav, ignore application (was 0% covered).
import { describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import type { InstalledApp } from "../types";

vi.mock("../lib/toast", () => ({
  toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() },
}));

import { useSoftwareController } from "../hooks/useSoftwareController";
import type { SoftwareControllerInput } from "../hooks/useSoftwareController";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import { appKey } from "../lib/appKey";

function app(name = "DemoApp"): InstalledApp {
  return {
    name,
    version: "1",
    publisher: "P",
    install_location: `C:\\Program Files\\${name}`,
    uninstall_string: "un.exe",
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: "k",
    estimated_size_kb: 0,
    install_date: "",
    display_icon: "",
  };
}

function input(overrides: Partial<SoftwareControllerInput> = {}): SoftwareControllerInput {
  const noop = () => {};
  return {
    apps: [app()],
    filtered: [app()],
    loading: false,
    q: "",
    category: "all",
    sortCol: "name",
    sortDesc: false,
    selected: null,
    multi: new Set<string>(),
    uninstallingKey: null,
    formatAppSize: () => "0 KB",
    sizeOf: () => 0,
    sortBy: noop,
    selectApp: noop,
    toggleMulti: noop,
    listStartUninstall: noop,
    listAnalyze: noop,
    listForceClean: noop,
    listIgnoreApp: noop,
    listIgnorePublisher: noop,
    estimating: false,
    scanning: false,
    aiEnabled: false,
    uninstallStage: null,
    showDetail: false,
    scan: null,
    selectedPaths: new Set<string>(),
    evidence: null,
    setEvidence: noop,
    setSelectedPaths: noop,
    ignoreSuggestions: [],
    residualFromUninstall: false,
    useOfficial: false,
    aiBusy: false,
    aiRisk: null,
    setAiRisk: noop,
    aiNotes: {},
    aiSummaryNote: null,
    aiNudgeDismissed: false,
    dryRunning: false,
    batching: false,
    error: null,
    report: null,
    aiReportBusy: false,
    aiReportNote: null,
    setAiReportBusy: noop,
    setAiReportNote: noop,
    verifyRows: null,
    checkup: { total: 0, large: 0, recent: 0 },
    checkupOpen: false,
    checkupOrphanCount: null,
    checkupOrphanBusy: false,
    batchIndex: 0,
    batchTotal: 0,
    batchCurrent: "",
    batchResults: [],
    showBatchSummary: false,
    setShowBatchSummary: noop,
    retryFailedBatch: noop,
    cancelBatch: noop,
    kindFilter: null,
    setKindFilter: noop,
    riskFilter: null,
    setRiskFilter: noop,
    detailPanel: null,
    core: {
      setSelected: noop,
      setMulti: noop,
      setIgnorePub: noop,
      setIgnoreName: noop,
      setUseOfficial: noop,
      setError: noop,
      setReport: noop,
    },
    residualActions: { clearIgnoreSuggestions: noop },
    shell: { setCheckupOpen: noop },
    shellActions: { toggleDetail: noop, closeCheckup: noop },
    setCategory: noop,
    stopSizeEstimate: async () => {},
    goNav: noop,
    dismissAiNudge: noop,
    closePreview: noop,
    dryRun: async () => {},
    handleCleanupConfirm: async () => {},
    runAiExplain: async () => {},
    checkupOrphanScan: noop,
    batchCleanup: async () => {},
    setQ: noop,
    setCopilotList: noop,
    setCategoryState: noop,
    L: t(),
    ...overrides,
  } as SoftwareControllerInput;
}

describe("useSoftwareController mapping (REV-QA-02)", () => {
  it("composes busy from dryRun/batch/scan/ai states", () => {
    const { result } = renderHook(() =>
      useSoftwareController(input({ dryRunning: true })),
    );
    expect(result.current.busy).toBe(true);
    const idle = renderHook(() => useSoftwareController(input())).result.current;
    expect(idle.busy).toBe(false);
  });

  it("onQuery updates the query and drops the copilot list", () => {
    const setQ = vi.fn();
    const setCopilotList = vi.fn();
    const { result } = renderHook(() => useSoftwareController(input({ setQ, setCopilotList })));
    result.current.onQuery("chrome");
    expect(setQ).toHaveBeenCalledWith("chrome");
    expect(setCopilotList).toHaveBeenCalledWith(null);
  });

  it("onCopilotApplyFilter applies the list and resets query/category", () => {
    const setCopilotList = vi.fn();
    const setQ = vi.fn();
    const setCategoryState = vi.fn();
    const { result } = renderHook(() =>
      useSoftwareController(input({ setCopilotList, setQ, setCategoryState })),
    );
    const list = [app("A"), app("B")];
    result.current.onCopilotApplyFilter(list);
    expect(setCopilotList).toHaveBeenCalledWith(list);
    expect(setQ).toHaveBeenCalledWith("");
    expect(setCategoryState).toHaveBeenCalledWith("all");
  });

  it("onCopilotBatch arms multi with the app keys and toasts", () => {
    const setMulti = vi.fn();
    const setCopilotList = vi.fn();
    const { result } = renderHook(() =>
      useSoftwareController(input({ core: { ...input().core, setMulti }, setCopilotList })),
    );
    const list = [app("A"), app("B")];
    result.current.onCopilotBatch(list);
    expect(setMulti).toHaveBeenCalledWith(new Set(list.map(appKey)));
    expect(setCopilotList).toHaveBeenCalledWith(list);
    expect(toast.info).toHaveBeenCalled();
  });

  it("onGoOrphans closes the checkup and navigates", () => {
    const goNav = vi.fn();
    const closeCheckup = vi.fn();
    const { result } = renderHook(() =>
      useSoftwareController(
        input({
          goNav,
          shellActions: { toggleDetail: vi.fn(), closeCheckup },
        }),
      ),
    );
    result.current.onGoOrphans();
    expect(closeCheckup).toHaveBeenCalled();
    expect(goNav).toHaveBeenCalledWith("orphans");
  });

  it("onIgnoreApplied stores rules and clears suggestions", () => {
    const setIgnorePub = vi.fn();
    const setIgnoreName = vi.fn();
    const clearIgnoreSuggestions = vi.fn();
    const { result } = renderHook(() =>
      useSoftwareController(
        input({
          core: { ...input().core, setIgnorePub, setIgnoreName },
          residualActions: { clearIgnoreSuggestions },
        }),
      ),
    );
    result.current.onIgnoreApplied(["Acme"], ["DemoApp"]);
    expect(setIgnorePub).toHaveBeenCalledWith(["Acme"]);
    expect(setIgnoreName).toHaveBeenCalledWith(["DemoApp"]);
    expect(clearIgnoreSuggestions).toHaveBeenCalled();
  });
});
