// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { setLang, t } from "../i18n";
import { ReportPanel } from "../components/ReportPanel";
import { SoftwareToolbar } from "../components/SoftwareToolbar";
import { ScanLeftoversView } from "../components/ScanLeftoversView";
import { OrphanOriginGroups } from "../components/OrphanOriginGroups";
import { groupByOrigin } from "../lib/decision";
import type { CleanupItem, FullCleanupReport } from "../types";
import { CleanupResultDetails } from "../components/CleanupResultDetails";
import { api } from "../lib/api";
vi.mock("@tanstack/react-virtual", () => ({ useVirtualizer: () => ({
  getVirtualItems: () => [{ index: 0, start: 0 }], getTotalSize: () => 72, measureElement: vi.fn(),
}) }));
afterEach(() => { cleanup(); setLang("zh"); });
it.each(["zh", "en"] as const)("separates pending reboot counts and never claims partial cleanup is complete in %s", lang => {
  setLang(lang);
  const base: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "",
    uninstall_ok: true, uninstall_message: "", deleted: 2, failed: 1, skipped: 3, delayed: 4,
    aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  const view = render(<ReportPanel report={base} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  for (const [label, count] of [[t().reportDeleted, 2], [t().reportFailed, 1], [t().reportSkipped, 3],
    [t().reportPendingReboot, 4]] as const) {
    expect(screen.getByText(label).parentElement?.firstElementChild?.textContent).toBe(String(count));
  }
  expect(screen.getByText(t().reportProgressLabel(2, 10, 20))).toBeTruthy();
  expect(screen.queryByText(t().reportFlowDone)).toBeNull();
  expect(screen.queryByText(t().reportProgressComplete)).toBeNull();
  view.unmount();
  for (const extra of [{ delayed: 1 }, { skipped: 1 }, { aborted: true }, { dry_run: true }]) {
    const report = { ...base, failed: 0, skipped: 0, delayed: 0, ...extra };
    const v = render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
      verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
    expect(screen.queryByText(t().reportProgressComplete)).toBeNull();
    expect(screen.queryByText(t().reportFlowDone)).toBeNull();
    if (report.delayed) expect(screen.getByText(t().reportNextDelayed)).toBeTruthy();
    v.unmount();
  }
});
it.each(["zh", "en"] as const)("states the backup prerequisite in the actual guide in %s", lang => {
  setLang(lang);
  render(<SoftwareToolbar q="" category="all" estimating={false} scanning={false} aiEnabled={false}
    smartFilterOpen={false} onQuery={() => {}} onCategory={() => {}} onStopEstimate={() => {}}
    onOpenAi={() => {}} onToggleSmartFilter={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: t().guided }));
  expect(screen.getByRole("note").textContent).toContain(lang === "zh" ? "未备份无法找回" : "Unbacked cleanup cannot be undone");
});
it.each([41, 80])("keeps tail failures and unknown verification results reachable for %s rows", count => {
  const report: FullCleanupReport = { app_name: "Vendor", dry_run: false, backup_dir: "",
    uninstall_ok: true, uninstall_message: "", deleted: count, failed: 0, skipped: 0,
    aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  const rows = Array.from({ length: count }, (_, index) => ({ path: `verify-${index}`, kind: "file",
    still_there: index >= count - 2, error: index === count - 1 ? "denied" : undefined }));
  render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={rows} onDismiss={() => {}} onRegenerate={() => {}} />);
  const unknown = screen.getByRole("img", { name: t().verifyUnknown });
  expect(unknown.parentElement?.textContent).toContain(`verify-${count - 1}`);
  expect(screen.getByRole("img", { name: t().verifyStillPresent }).parentElement?.textContent)
    .toContain(`verify-${count - 2}`);
  expect(unknown.parentElement?.parentElement?.style.maxHeight).toBe("120px");
  expect(unknown.parentElement?.parentElement?.style.overflow).toBe("auto");
});
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
      { path: "unknown", kind: "registry", still_there: true, error: "access denied" },
    ]} onDismiss={() => {}} onRegenerate={() => {}} />);
  expect(screen.getByRole("img", { name: L.verifyStillPresent })).toBeTruthy();
  expect(screen.getByRole("img", { name: L.verifyRemoved })).toBeTruthy();
  expect(screen.getByRole("img", { name: L.verifyUnknown })).toBeTruthy();
  expect(screen.getByRole("img", { name: L.verifyUnknown }).parentElement?.textContent).toContain(L.verifyUnknown);
  expect(screen.getByText(L.restorePointFail).style.color).toBe("var(--warn-ink)");
  expect(screen.getByText(L.backendUninstallExit("1603"))).toBeTruthy();
  expect(container.textContent).toContain(L.backendDetailUnavailable);
  const details = screen.getByText(L.backendTechnicalDetails).closest("details")!;
  expect(details.open).toBe(false);
  expect(details.textContent).toContain(report.uninstall_message);
  expect(details.textContent).toContain("native <error>");
  expect(container.querySelector("error")).toBeNull();
});

it.each(["zh", "en"] as const)("groups results, exposes tail failures and reports location errors safely in %s", async lang => {
  setLang(lang);
  const open = vi.spyOn(api, "openPath").mockRejectedValue("open_path:not_found");
  const path = "C:\\Fixture\\long-name.txt";
  const items = Array.from({ length: 51 }, (_, n) => ({ path: n === 50 ? path : `C:\\Fixture\\${n}.txt`,
    kind: "file", status: "failed", message: n === 50 ? "Access is denied. (os error 5)" : "os error 32" }));
  items.push({ path: "HKCU\\Software\\Fixture", kind: "registry", status: "failed", message: "protected path" });
  items.push({ path: "https://example.test/unsafe", kind: "file", status: "failed", message: "os error 5" });
  items.push({ path: "C:\\Fixture\\pending", kind: "file", status: "delayed", message: "reboot delete" });
  render(<CleanupResultDetails items={items} />);
  expect(screen.getByText(`${t().reportFailed} · 53`).closest("details")?.open).toBe(true);
  expect(screen.getByText(`${t().reportPendingReboot} · 1`).closest("details")?.open).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: t().reportShowMore(3) }));
  expect(screen.getByText(path)).toBeTruthy();
  const missing = screen.getByRole("button", { name: `${t().openLocation}: ${path}` });
  fireEvent.click(missing);
  expect((await screen.findByRole("alert")).textContent).toContain(t().errOpenPathMissing);
  expect(open).toHaveBeenCalledWith(path);
  for (const target of ["HKCU\\Software\\Fixture", "https://example.test/unsafe"]) {
    expect(screen.getByRole("button", { name: `${t().openLocation}: ${target}` }).hasAttribute("disabled")).toBe(true);
  }
  expect(screen.getAllByText(t().reportFileBusy).length).toBe(50);
  expect(screen.getAllByText(t().reportPermissionDenied).length).toBe(2);
  open.mockRestore();
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
