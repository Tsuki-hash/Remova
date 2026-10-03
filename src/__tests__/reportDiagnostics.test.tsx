// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { setLang, t } from "../i18n";
import { ReportPanel } from "../components/ReportPanel";
import { ScanLeftoversView } from "../components/ScanLeftoversView";
import { OrphanOriginGroups } from "../components/OrphanOriginGroups";
import { groupByOrigin } from "../lib/decision";
import type { CleanupItem, FullCleanupReport } from "../types";
vi.mock("@tanstack/react-virtual", () => ({ useVirtualizer: () => ({
  getVirtualItems: () => [{ index: 0, start: 0 }], getTotalSize: () => 72, measureElement: vi.fn(),
}) }));
afterEach(() => { cleanup(); setLang("zh"); });
it.each(["zh", "en"] as const)("offers backup restoration only for a completed backed-up cleanup in %s", lang => {
  setLang(lang);
  const base: FullCleanupReport = { app_name: "Vendor", dry_run: false, backup_dir: "",
    uninstall_ok: true, uninstall_message: "", deleted: 1, failed: 0, skipped: 0,
    aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  for (const report of [base, { ...base, backup_dir: "session" },
    { ...base, backup_dir: "session", aborted: true },
    { ...base, failed: 1 }, { ...base, dry_run: true }]) {
    const view = render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false}
      aiReportNote={null} verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
    const hasRestoration = !!report.backup_dir && !report.aborted && !report.dry_run && !report.failed;
    expect(screen.queryByText(t().reportNextOk) !== null).toBe(hasRestoration);
    if (!report.backup_dir && !report.dry_run) expect(screen.getByText(t().noBackupThisRun)).toBeTruthy();
    view.unmount();
  }
});
it.each(["zh", "en"] as const)("keeps raw diagnostics in technical details while localizing the visible report in %s", lang => {
  setLang(lang);
  const L = t();
  const report: FullCleanupReport = { app_name: "Vendor", dry_run: false, backup_dir: "",
    uninstall_ok: false, uninstall_message: "uninstaller exited with Some(1603)",
    deleted: 0, failed: 1, skipped: 0, aborted: true, restore_point_ok: false,
    restore_point_msg: "SRSetRestorePointW failed", errors: ["native <error>"],
    item_details: [{ path: "fixture", kind: "file", status: "failed", message: "unknown <diagnostic>" }] };
  const { container } = render(<ReportPanel report={report} aiEnabled={false}
    aiReportBusy={false} aiReportNote={null} verifyRows={[
      { path: "remaining", kind: "file", still_there: true },
      { path: "removed", kind: "file", still_there: false },
    ]} onDismiss={() => {}} onRegenerate={() => {}} />);
  expect(screen.getByRole("img", { name: L.verifyStillPresent })).toBeTruthy();
  expect(screen.getByRole("img", { name: L.verifyRemoved })).toBeTruthy();
  expect(screen.getByText(L.restorePointFail).style.color).toBe("var(--warn-ink)");
  expect(screen.getByText(L.backendUninstallExit("1603"))).toBeTruthy();
  expect(container.textContent).toContain(L.backendDetailUnavailable);
  const details = container.querySelector("details")!;
  expect(details.open).toBe(false);
  expect(details.textContent).toContain(report.uninstall_message);
  expect(details.textContent).toContain("native <error>");
  expect(container.querySelector("error")).toBeNull();
});

it.each(["zh", "en"] as const)("names evidence buttons by action and path in %s", lang => {
  setLang(lang);
  const onEvidence = vi.fn();
  render(<ScanLeftoversView scan={{ app_name: "Vendor", items: [{ path: "fixture-path",
    kind: "file", score: 90, confidence: "confirmed", risk: "low", reason: "fixture", evidence: [] }] }}
    scanning={false} selectedPaths={new Set()} evidence={null} aiNotes={{}} orphanLabel="Orphans"
    onTogglePath={() => {}} onEvidence={onEvidence} />);
  fireEvent.click(screen.getByRole("button", { name: `${t().orphanEvidenceTitle}: fixture-path` }));
  expect(onEvidence).toHaveBeenCalledWith("fixture");
});

it.each(["zh", "en"] as const)("uses translated evidence in both leftover surfaces in %s", lang => {
  setLang(lang);
  const evidence = [{ code: "orphan_no_owner", label: "No matching uninstall entry", weight: 30,
    detail: "folder `Vendor` not matched to installed software" }];
  const item: CleanupItem = { path: "fixture-path", kind: "dir", score: 30,
    confidence: "suspected", risk: "medium", reason: "orphan", evidence };
  const onEvidence = vi.fn();
  const view = render(<ScanLeftoversView scan={{ app_name: "Vendor", items: [item] }}
    scanning={false} selectedPaths={new Set()} evidence={null} aiNotes={{}} orphanLabel="Orphans"
    onTogglePath={() => {}} onEvidence={onEvidence} />);
  fireEvent.click(screen.getByRole("button", { name: `${t().orphanEvidenceTitle}: fixture-path` }));
  expect(onEvidence).toHaveBeenCalledWith(`${t().orphanEvNoOwner} (30)`);
  view.unmount();
  render(<OrphanOriginGroups groups={groupByOrigin([item])} selectedPaths={new Set()}
    onToggle={() => {}} onToggleMany={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: t().orphanExpandEvidence }));
  expect(screen.getByText(t().orphanEvNoOwner)).toBeTruthy();
  expect(screen.queryByText("No matching uninstall entry")).toBeNull();
  const technical = screen.getByText(t().backendTechnicalDetails).closest("details")!;
  expect(technical.open).toBe(false);
  expect(technical.textContent).toContain(evidence[0]!.detail);
});
