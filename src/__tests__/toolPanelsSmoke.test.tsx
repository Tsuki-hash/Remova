// @vitest-environment jsdom
// tool panel smoke — empty/diff surfaces render and fire callbacks.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { MonitorPanel } from "../components/MonitorPanel";
import { HistoryPanel } from "../components/HistoryPanel";
import { CheckupPanel } from "../components/CheckupPanel";
import { ScanActionsBar } from "../components/ScanActionsBar";
import { t, formatSize } from "../i18n";
import type { CleanupItem } from "../types";

afterEach(cleanup);

describe("MonitorPanel", () => {
  it("shows degradation and blocks cleanup even when rows exist", () => {
    const onToCleanup = vi.fn();
    render(<MonitorPanel diff={{ added_files: ["test-file"], added_reg_values: [], walk_degraded: true }} onToCleanup={onToCleanup} onDismiss={() => {}} />);
    expect(screen.getByText(/追踪扫描被截断/)).toBeTruthy();
    const btn = screen.getByRole("button", { name: "转入清理列表" });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(btn);
    expect(onToCleanup).not.toHaveBeenCalled();
  });
  it("shows empty state and disables cleanup CTA", () => {
    const onToCleanup = vi.fn();
    const { container } = render(
      <MonitorPanel
        diff={{ added_files: [], added_reg_values: [] }}
        monitoring={false}
        onToCleanup={onToCleanup}
        onDismiss={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.getByText(/暂无安装变更|No install changes/)).toBeTruthy();
    const btn = within(container).getByRole("button", { name: "转入清理列表" });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(btn);
    expect(onToCleanup).not.toHaveBeenCalled();
  });

  it("lists diff rows and enables cleanup", () => {
    const onToCleanup = vi.fn();
    const { container } = render(
      <MonitorPanel
        diff={{ added_files: ["C:\\new\\a.dll"], added_reg_values: ["HKCU\\x"] }}
        monitoring
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
    const item: CleanupItem = { path: "C:\Tool", kind: "file", size_kb: 10, risk: "low", confidence: "confirmed", score: 90, reason: "", evidence: [] };
    render(<ScanActionsBar {...baseProps} scan={{ ...baseScan, items: [item] }} selectedPaths={new Set(["C:\Tool"])} scanning={true} />);
    expect(screen.getByText(t().analyzing)).toBeTruthy();
    expect(screen.queryByText(t().cleanupPlanReview(1))).toBeNull();
    expect(screen.queryByText(t().conclusionSpace(formatSize(10)))).toBeNull();
  });

  it("renders the AI note with its disclaimer", () => {
    render(<ScanActionsBar {...baseProps} aiNote="AI note text" />);
    expect(screen.getByText(/AI note text/)).toBeTruthy();
    expect(screen.getByText(new RegExp(t().aiDisclaimer))).toBeTruthy();
  });

  it("shows the action-row estimate once a row is selected", () => {
    const item: CleanupItem = { path: "C:\\Tool", kind: "file", size_kb: 10, risk: "low", confidence: "confirmed", score: 90, reason: "", evidence: [] };
    render(<ScanActionsBar {...baseProps} scan={{ ...baseScan, items: [item] }} selectedPaths={new Set(["C:\\Tool"])} />);
    expect(screen.getByText(`${t().cleanup} (1)`)).toBeTruthy();
    expect(screen.getByText(t().conclusionSpace(formatSize(10)))).toBeTruthy();
    expect(screen.queryByText(t().cleanupPlanReview(0))).toBeNull();
  });
});
