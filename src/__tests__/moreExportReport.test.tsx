// @vitest-environment jsdom
// the More page's "export report" card promised the *analysis* report while
// it actually exports the cleanup result, and its "New" badge never went away for
// as long as `lastReport` was retained.
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MorePage } from "../components/MorePage";
import { t } from "../i18n";
import type { FullCleanupReport } from "../types";

vi.mock("../lib/api", () => ({ api: { openPath: vi.fn().mockResolvedValue(undefined), rebootGateStatus: vi.fn().mockResolvedValue("clear") } }));
afterEach(cleanup);

const report: FullCleanupReport = {
  app_name: "Acme", dry_run: false, backup_dir: "", uninstall_ok: false, uninstall_message: "",
  deleted: 1, failed: 0, skipped: 0, aborted: false, restore_point_ok: false, restore_point_msg: "",
  errors: [], item_details: [],
};

const view = (lastReport: FullCleanupReport | null) => render(
  <MorePage
    selected={null}
    monitoring={false}
    monitorDiff={null}
    lastReport={lastReport}
    closeMode={null}
    onCloseModeChange={vi.fn()}
    onIgnorePublisher={vi.fn()}
    onToggleMonitor={vi.fn()}
    onMonitorToCleanup={vi.fn()}
    onDismissMonitor={vi.fn()}
    onExportReport={vi.fn()}
    onError={vi.fn()}
    onCheckUpdate={vi.fn()}
    onGoSoftware={vi.fn()}
  />,
);

it("describes the export card as the cleanup result, not the analysis", () => {
  view(null);
  // The hint names the cleanup result and does not claim it is the analysis.
  expect(t().exportReportHint).toMatch(/清理结果|cleanup result/i);
  expect(t().exportReportHint).not.toMatch(/分析报告|analysis report/i);
});

it("shows the New badge only for the first report of the session", () => {
  const L = t();
  const first = view(report);
  const card = screen.getByRole("button", { name: new RegExp(L.exportReport) });
  expect(card.textContent).toContain(L.badgeNew);

  // The same page re-rendering with the report still retained must drop it.
  first.rerender(
    <MorePage
      selected={null}
      monitoring={false}
      monitorDiff={null}
      lastReport={report}
      closeMode={null}
      onCloseModeChange={vi.fn()}
      onIgnorePublisher={vi.fn()}
      onToggleMonitor={vi.fn()}
      onMonitorToCleanup={vi.fn()}
      onDismissMonitor={vi.fn()}
      onExportReport={vi.fn()}
      onError={vi.fn()}
      onCheckUpdate={vi.fn()}
      onGoSoftware={vi.fn()}
    />,
  );
  expect(screen.getByRole("button", { name: new RegExp(L.exportReport) }).textContent)
    .not.toContain(L.badgeNew);
  cleanup();

  // A fresh mount is a fresh session: the next report is new again.
  view(report);
  expect(screen.getByRole("button", { name: new RegExp(L.exportReport) }).textContent)
    .toContain(L.badgeNew);
});

it("hides the export card entirely until a report exists", () => {
  view(null);
  expect(screen.queryByRole("button", { name: new RegExp(t().exportReport) })).toBeNull();
});
