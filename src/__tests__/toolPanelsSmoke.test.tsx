// @vitest-environment jsdom
// tool panel smoke — empty/diff surfaces render and fire callbacks.
import { cleanupItemKey } from "../lib/cleanupPlan";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { MonitorPanel } from "../components/MonitorPanel";
import { InstallTrialHistory } from "../components/InstallTrialHistory";
import { api } from "../lib/api";
import { HistoryPanel } from "../components/HistoryPanel";
import { CheckupPanel } from "../components/CheckupPanel";
import { ScanProgressBar } from "../components/ScanProgressBar";
import { ScanActionsBar } from "../components/ScanActionsBar";
import { t, formatSize, setLang } from "../i18n";
import type { CleanupItem, InstalledApp } from "../types";

afterEach(cleanup);

describe("MonitorPanel", () => {
  it("stops tracking from the active tool without reopening the directory", () => {
    const L = t();
    const onToggle = vi.fn();
    const actions = { onToggle, onToCleanup: vi.fn(), onDismiss: vi.fn() };
    const { rerender } = render(
      <MonitorPanel diff={null} monitoring {...actions} />,
    );
    expect(screen.getByRole("status").textContent).toBe(L.monitorActive);
    expect(screen.queryByRole("button", { name: L.monitorToCleanup })).toBeNull();
    expect(screen.queryByRole("group", { name: L.monitorDiff })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: L.monitorStop }));
    expect(onToggle).toHaveBeenCalledOnce();
    rerender(<MonitorPanel diff={null} monitoring stopping {...actions} />);
    expect(screen.getByRole("button", { name: L.monitorStop })).toHaveProperty("disabled", true);
    expect(screen.queryByText(L.monitorActive)).toBeNull();
  });

  it("offers a retry after tracking could not start", () => {
    const L = t();
    const onToggle = vi.fn();
    render(<MonitorPanel diff={null} onToggle={onToggle} onToCleanup={vi.fn()} onDismiss={vi.fn()} />);
    expect(screen.getByRole("status").textContent).toBe(L.monitorStartHint);
    fireEvent.click(screen.getByRole("button", { name: L.monitorStartAction }));
    expect(onToggle).toHaveBeenCalledOnce();
  });

  it("passes the optional name and presents the finished trial without a cleanup action", () => {
    const L = t();
    const onToggle = vi.fn();
    const props = { onToggle, onToCleanup: vi.fn(), onDismiss: vi.fn() };
    const { rerender } = render(<MonitorPanel diff={null} {...props} />);
    fireEvent.change(screen.getByRole("textbox", { name: L.trialName }), { target: { value: "  Editor trial  " } });
    fireEvent.click(screen.getByRole("button", { name: L.monitorStartAction }));
    expect(onToggle).toHaveBeenCalledWith("Editor trial");
    rerender(<MonitorPanel diff={{ added_files: ["editor.exe"], added_reg_values: [], trial_read_only: true }} {...props} />);
    expect(screen.getByText(L.trialReadOnly)).toBeTruthy();
    expect(screen.queryByRole("button", { name: L.monitorToCleanup })).toBeNull();
  });

  it("shows matching trial records alongside concurrent changes with unconfirmed ownership", async () => {
    const L = t();
    const registry = "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Editor";
    const app: InstalledApp = {
      name: "Editor", registry_key: registry, install_location: "D:/Apps/Editor", source: "HKCU",
      version: "", publisher: "", uninstall_string: "", quiet_uninstall_string: "",
      estimated_size_kb: 0, install_date: "", display_icon: "",
    };
    const trial = { id: "1", name: "Editor trial", started_at: 1, file_roots: ["D:\\Apps"], registry_roots: ["HKCU"] };
    const list = vi.spyOn(api, "installTrialRecords").mockResolvedValue([
      { trial, finished_at: 2, diff: {
        added_files: ["D:\\Apps\\Editor\\cache", "D:\\Apps\\EditorUpdate\\cache"],
        added_reg_values: [registry.toLowerCase()], files_truncated: 3, recovered_read_only: true,
      } },
      { trial: { ...trial, id: "2", name: "Unrelated trial" }, finished_at: 2, diff: {
        added_files: ["D:\\Apps\\EditorUpdate\\cache"], added_reg_values: [`${registry}-other`],
      } },
    ]);
    try {
      render(<InstallTrialHistory app={app} />);
      expect(list).not.toHaveBeenCalled();
      const history = screen.getByText(L.trialHistory).closest("details")!;
      history.open = true;
      fireEvent(history, new Event("toggle"));
      expect(await screen.findByText(trial.name)).toBeTruthy();
      expect(screen.queryByText("Unrelated trial")).toBeNull();
      expect(screen.getByText(L.trialRegistry)).toBeTruthy();
      expect(screen.getByText(L.trialDirectory)).toBeTruthy();
      expect(screen.getByText(L.trialUnconfirmed)).toBeTruthy();
      expect(screen.getByText(L.monitorTruncated(3, 0))).toBeTruthy();
      expect(screen.getByText(L.monitorRecoveryReadOnly)).toBeTruthy();
    } finally {
      list.mockRestore();
    }
  });

  it("shows degradation and blocks cleanup even when rows exist", () => {
    const onToCleanup = vi.fn();
    render(<MonitorPanel diff={{ added_files: ["test-file"], added_reg_values: [], walk_degraded: true }} onToCleanup={onToCleanup} onDismiss={() => {}} />);
    expect(screen.getByText(/追踪扫描被截断/)).toBeTruthy();
    const btn = screen.getByRole("button", { name: "转入清理列表" });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(btn);
    expect(onToCleanup).not.toHaveBeenCalled();
  });
  it("shows the empty result without a cleanup action or an empty list to focus", () => {
    const L = t();
    render(
      <MonitorPanel
        diff={{ added_files: [], added_reg_values: [] }}
        monitoring={false}
        onToCleanup={vi.fn()}
        onDismiss={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByRole("status").textContent).toBe(L.monitorEmpty);
    expect(screen.queryByRole("button", { name: L.monitorToCleanup })).toBeNull();
    expect(screen.queryByRole("group", { name: L.monitorDiff })).toBeNull();
  });

  it("lists diff rows and enables cleanup", () => {
    const onToCleanup = vi.fn();
    const { container } = render(
      <MonitorPanel
        diff={{ added_files: ["C:\\new\\a.dll"], added_reg_values: ["HKCU\\x"] }}
        onToCleanup={onToCleanup}
        onDismiss={() => {}}
      />,
    );
    expect(screen.getByText("C:\\new\\a.dll")).toBeTruthy();
    const btn = within(container).getByRole("button", { name: "转入清理列表" });
    expect((btn as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(btn);
    expect(onToCleanup).toHaveBeenCalled();
  });
});

describe("HistoryPanel", () => {
  it("renders empty list without crashing and close works", () => {
    const onClose = vi.fn();
    const { container } = render(
      <HistoryPanel
        history={[]}
        histQ=""
        setHistQ={() => {}}
        onDelete={() => {}}
        onClearAll={() => {}}
        onClose={onClose}
      />,
    );
 // CloseGlyph is aria-hidden — the host button is the last action in the header.
    const buttons = within(container).getAllByRole("button");
    const close = buttons[buttons.length - 1]!;
    fireEvent.click(close);
    expect(onClose).toHaveBeenCalled();
  });

  it("filters by query without throwing on day grouping", () => {
    render(
      <HistoryPanel
        history={[
          {
            id: "h1",
            app_name: "DemoApp",
            deleted: 2,
            failed: 0,
            skipped: 0,
            backup_dir: "C:\\bak",
            created_at: "1700000000",
          },
        ]}
        histQ="Demo"
        setHistQ={() => {}}
        onDelete={() => {}}
        onClearAll={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByText(/DemoApp/)).toBeTruthy();
  });
});

describe("CheckupPanel", () => {
  it("keeps cancellation inside the modal focus scope and distinguishes incomplete results", () => {
    const cancel = vi.fn(async () => {});
    const props = { stats: { total: 3, large: 1, recent: 0 }, orphanScanning: true,
      orphanCount: null, onClose: vi.fn(), onOrphanScan: vi.fn(), onOpenOrphans: vi.fn() };
    const view = render(<CheckupPanel {...props} scanStatus={<ScanProgressBar
      progress={{ status: "running", stage: "files", found: 4, elapsedMs: 300 }} onCancel={cancel} />} />);
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: t().cancelScan }));
    expect(cancel).toHaveBeenCalledOnce();
    view.rerender(<CheckupPanel {...props} orphanScanning={false} scanStatus={<ScanProgressBar
      progress={{ status: "cancelled", stage: "files", found: 4, elapsedMs: 900 }} onCancel={cancel} />} />);
    expect(within(dialog).getByRole("status").textContent).toBe(t().scanCancelledIncomplete);
    expect(within(dialog).getByRole("button", { name: t().navOrphans }).hasAttribute("disabled")).toBe(true);
  });
  it("shows stats and Escape closes", () => {
    const onClose = vi.fn();
    render(
      <CheckupPanel
        stats={{ total: 3, large: 1, recent: 0 }}
        orphanScanning={false}
        orphanCount={null}
        onClose={onClose}
        onOrphanScan={() => {}}
        onOpenOrphans={() => {}}
      />,
    );
    expect(screen.getByText("3")).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});

describe("ScanActionsBar", () => {
  it.each(["zh", "en"] as const)("puts direct cleanup behind advanced options with an explicit warning (%s)", (lang) => {
    setLang(lang);
    try {
      const onUseOfficial = vi.fn();
      const props = { ...baseProps, useOfficial: true, selectedPaths: new Set<string>(), onUseOfficial };
      const view = render(<ScanActionsBar {...props} />);
      expect(screen.getByText(t().officialFirstHint)).toBeTruthy();
      expect(screen.getByRole("button", { name: t().uninstallAndCleanup }).title).toBe(t().officialFirstHint);
      const skip = screen.getByLabelText<HTMLInputElement>(t().skipOfficial);
      expect(skip.checked).toBe(false);
      expect(skip.closest("details")!.open).toBe(false);
      expect(skip.closest("details")!.textContent).toContain(t().skipOfficialHint);
      fireEvent.click(skip);
      expect(onUseOfficial).toHaveBeenLastCalledWith(false);
      view.rerender(<ScanActionsBar {...props} useOfficial={false} />);
      expect(skip.checked).toBe(true);
      expect(screen.queryByRole("button", { name: t().uninstallAndCleanup })).toBeNull();
      expect(screen.getByRole("button", { name: `${t().cleanup} (0)` }).title).toBe(t().cleanupSelectedHint);
      expect(view.container.querySelector(".scan-actions-secondary")!.textContent).toContain(t().skipOfficialHint);
      fireEvent.click(skip);
      expect(onUseOfficial).toHaveBeenLastCalledWith(true);
      for (const state of [{ busy: true }, { dryRunning: true }, { scanning: true }]) {
        view.rerender(<ScanActionsBar {...props} {...state} />);
        expect(skip.disabled).toBe(true);
      }
      for (const state of [{ residualFromUninstall: true }, { canRunOfficial: false }]) {
        view.rerender(<ScanActionsBar {...props} {...state} />);
        expect(screen.queryByLabelText(t().skipOfficial)).toBeNull();
        expect(screen.queryByText(t().officialFirstHint)).toBeNull();
        expect(screen.queryByRole("button", { name: t().uninstallAndCleanup })).toBeNull();
        expect(screen.getByRole("button", { name: `${t().cleanup} (0)` }).title).toBe(t().cleanupSelectedHint);
      }
    } finally {
      setLang("zh");
    }
  });
  it.each(["zh", "en"] as const)("shows plan checks beside cleanup and preserves busy guards (%s)", (lang) => {
    setLang(lang);
    try {
      const item: CleanupItem = { path: "fixture", kind: "file", size_kb: 10, risk: "low", confidence: "confirmed", score: 90, reason: "", evidence: [] };
      const onDryRun = vi.fn();
      const props = { ...baseProps, scan: { ...baseScan, items: [item] }, selectedPaths: new Set([cleanupItemKey(item)]), onDryRun };
      const view = render(<ScanActionsBar {...props} />);
      const disclosure = screen.getByText(t().cleanupPlanDetails).closest("details")!;
      expect(disclosure.open).toBe(false);
      const check = screen.getByRole("button", { name: t().dryRun });
      expect(disclosure.contains(check)).toBe(false);
      expect(view.container.querySelector(".scan-actions-primary")!.contains(check)).toBe(true);
      expect(screen.getByText(t().dryRunHint).id).toBe(check.getAttribute("aria-describedby"));
      fireEvent.click(check);
      expect(onDryRun).toHaveBeenCalledOnce();
      for (const state of [{ busy: true }, { scanning: true }, { dryRunning: true }, { selectedPaths: new Set<string>() }]) {
        view.rerender(<ScanActionsBar {...props} {...state} />);
        expect((check as HTMLButtonElement).disabled).toBe(true);
        fireEvent.click(check);
      }
      expect(onDryRun).toHaveBeenCalledOnce();
    } finally {
      setLang("zh");
    }
  });
  const baseScan = { app_name: "App", items: [] };
  const baseProps = {
    scan: baseScan,
    scanning: false,
    dryRunning: false,
    residualFromUninstall: false,
    useOfficial: false,
    aiEnabled: false,
    aiBusy: false,
    busy: false,
    onBack: vi.fn(),
    onUseOfficial: vi.fn(),
    onDryRun: vi.fn(),
    onCleanup: vi.fn(),
    onAiExplain: vi.fn(),
  };

  it("keeps the cleanup CTA disabled and estimate numbers hidden while nothing is selected", () => {
    render(<ScanActionsBar {...baseProps} selectedPaths={new Set()} />);
    expect(screen.getByRole("button", { name: `${t().cleanup} (0)` }).hasAttribute("disabled")).toBe(true);
    expect(screen.queryByText(t().cleanupPlanReview(0))).toBeNull();
    expect(screen.queryByText(t().conclusionSpace(formatSize(0)))).toBeNull();
  });

  it("shows the analyzing state and hides estimates while scanning", () => {
    const item: CleanupItem = { path: "C:\\Tool", kind: "file", size_kb: 10, risk: "low", confidence: "confirmed", score: 90, reason: "", evidence: [] };
    render(<ScanActionsBar {...baseProps} scan={{ ...baseScan, items: [item] }} selectedPaths={new Set([cleanupItemKey(item)])} scanning={true} />);
    expect(screen.getByText(t().analyzing)).toBeTruthy();
    expect(screen.queryByText(t().cleanupPlanReview(1))).toBeNull();
    expect(screen.queryByText(t().conclusionSpace(formatSize(10)))).toBeNull();
  });

  it("renders the AI note with its disclaimer", () => {
    render(<ScanActionsBar {...baseProps} selectedPaths={new Set()} aiNote="AI note text" />);
    expect(screen.getByText(/AI note text/)).toBeTruthy();
    expect(screen.getByText(new RegExp(t().aiDisclaimer))).toBeTruthy();
  });

  it("shows the action-row estimate once a row is selected", () => {
    const item: CleanupItem = { path: "C:\\Tool", kind: "file", size_kb: 10, risk: "low", confidence: "confirmed", score: 90, reason: "", evidence: [] };
    render(<ScanActionsBar {...baseProps} scan={{ ...baseScan, items: [item] }} selectedPaths={new Set([cleanupItemKey(item)])} />);
    expect(screen.getByText(`${t().cleanup} (1)`)).toBeTruthy();
    expect(screen.getByText(t().conclusionSpace(formatSize(10)))).toBeTruthy();
    expect(screen.queryByText(t().cleanupPlanReview(0))).toBeNull();
  });
});
