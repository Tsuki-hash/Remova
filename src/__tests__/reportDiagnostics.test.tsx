// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { setLang, t } from "../i18n";
import { ReportPanel } from "../components/ReportPanel";
import { SoftwareToolbar } from "../components/SoftwareToolbar";
import { ScanLeftoversView } from "../components/ScanLeftoversView";
import { cleanupItemKey } from "../lib/cleanupPlan";
import { OrphanOriginGroups } from "../components/OrphanOriginGroups";
import { groupByOrigin } from "../lib/decision";
import type { CleanupItem, CleanupReport, FullCleanupReport } from "../types";
import { CleanupResultDetails } from "../components/CleanupResultDetails";
import { api } from "../lib/api";
import { useBackupSessionRequest, clearBackupSessionRequest } from "../lib/backupSessionNavigation";
vi.mock("@tanstack/react-virtual", () => ({ useVirtualizer: () => ({
  getVirtualItems: () => [{ index: 0, start: 0 }], getTotalSize: () => 72, measureElement: vi.fn(),
}) }));
afterEach(() => { cleanup(); setLang("zh"); });
it.each(["required", "before_verification"] as const)("keeps %s restart guidance after residual cleanup", restart => {
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "", uninstall_ok: false,
    uninstall_message: "", deleted: 1, failed: 0, skipped: 0, aborted: false,
    restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  render(<ReportPanel report={report} residualRestart={restart} aiEnabled={false} aiReportBusy={false}
    aiReportNote={null} verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  const expected = restart === "required" ? t().backendRebootRequired : t().backendUninstallRebootBeforeVerify;
  expect(screen.getByText(expected).getAttribute("role")).toBe("status");
});
it.each(["zh", "en"] as const)("brings planned and skipped counts into view without claiming cleanup in %s", lang => {
  setLang(lang);
  const L = t();
  const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");
  const scroll = vi.fn();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: scroll });
  try {
    const preview: CleanupReport = { app_name: "sample", dry_run: true, deleted_planned: 7, skipped: 2,
      uninstall_command: [], uninstall_message: "", errors: [], item_details: [] };
    const fullPreview: FullCleanupReport = { app_name: "sample", dry_run: true, backup_dir: "", uninstall_ok: false,
      uninstall_message: "", deleted: 7, failed: 0, skipped: 2, aborted: false,
      restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
    const props = { aiEnabled: false, aiReportBusy: false, aiReportNote: null,
      verifyRows: null, onDismiss: () => {}, onRegenerate: () => {} };
    const view = render(<ReportPanel {...props} report={preview} />);
    for (const report of [preview, fullPreview]) {
      view.rerender(<ReportPanel {...props} report={report} />);
      const status = screen.getByRole("status");
      expect(status.textContent).toContain(`${L.dryRunPlanned} 7`);
      expect(status.textContent).toContain(`${L.reportSkipped} 2`);
      expect(status.closest("details")).toBeNull();
      expect(screen.getByText(L.cleanupReportDetails).closest("details")?.open).toBe(false);
      expect(screen.queryByText(`${L.reportDeleted} 7`, { exact: false })).toBeNull();
    }
    expect(scroll).toHaveBeenCalledTimes(2);
    expect(scroll).toHaveBeenLastCalledWith({ block: "start", behavior: "auto" });
  } finally {
    if (original) Object.defineProperty(HTMLElement.prototype, "scrollIntoView", original);
    else Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  }
});

it.each(["zh", "en"] as const)("separates official success from remaining scan status in %s", lang => {
  setLang(lang);
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "", uninstall_ok: true,
    uninstall_message: "", deleted: 0, failed: 1, skipped: 0, delayed: 0, aborted: false,
    restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  const props = { report, aiEnabled: false, aiReportBusy: false, aiReportNote: null,
    verifyRows: null, onDismiss: () => {}, onRegenerate: () => {} };
  const view = render(<ReportPanel {...props} residualScan={{ phase: "scanning" }} />);
  expect(screen.getByRole("status").textContent).toContain(t().uninstallOk);
  expect(screen.getByRole("status").textContent).toContain(t().cleanupRescanRunning);
  for (const count of [0, 2, undefined]) {
    view.rerender(<ReportPanel {...props} residualScan={count === undefined ? undefined : { phase: "complete", count }} />);
    expect(screen.getByRole("status").textContent).toContain(count === undefined ? t().cleanupRescanUnknown : t().cleanupRescanResult(count));
    expect(screen.queryByText(t().reportFlowDone)).toBeNull();
  }
  view.rerender(<ReportPanel {...props} report={{ ...report, aborted: true }} residualScan={{ phase: "scanning" }} />);
  expect(screen.getByRole("status").textContent).toBe(t().uninstallOk);
  expect(screen.queryByText(t().reportFlowDone)).toBeNull();
});
it.each(["zh", "en"] as const)("shows a history write warning alongside a successful cleanup in %s", lang => {
  setLang(lang);
  const error = "cleanup history could not be saved: history compaction failed: access denied";
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "", uninstall_ok: true,
    uninstall_message: "", deleted: 1, failed: 0, skipped: 0, aborted: false,
    restore_point_ok: false, restore_point_msg: "", errors: [error], item_details: [] };
  render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  expect(screen.getByText(t().uninstallOk)).toBeTruthy();
  expect(screen.getByText(t().cleanupHistorySaveFailed).getAttribute("role")).toBe("status");
  expect(screen.getByText(error).tagName).toBe("PRE");
});
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

it.each(["zh", "en"] as const)("keeps reboot guidance visible and focuses the pending group in %s", lang => {
  setLang(lang);
  const L = t();
  const onRescan = vi.fn();
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "", uninstall_ok: true,
    uninstall_message: "", deleted: 0, failed: 0, skipped: 0, delayed: 1, aborted: false,
    restore_point_ok: false, restore_point_msg: "", errors: [],
    item_details: [{ path: "C:\\Sample\\pending.bin", kind: "file", status: "delayed", message: "reboot delete" }] };
  render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={null} onRescan={onRescan} onDismiss={() => {}} onRegenerate={() => {}} />);
  const details = screen.getByText(L.cleanupReportDetails).closest("details")!;
  const pending = screen.getByText(`${L.reportPendingReboot} · 1`);
  expect(details.open).toBe(false);
  expect(screen.getByText(L.reportNextDelayed).closest("details")).toBeNull();
  expect(pending.closest("details")?.open).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: L.reportViewDelayed }));
  expect(details.open).toBe(true);
  expect(pending.closest("details")?.open).toBe(true);
  expect(document.activeElement).toBe(pending);
  expect(onRescan).not.toHaveBeenCalled();
});

it.each([
  { scenario: "known file lock", count: 1, reasons: ["sharing violation (os error 32)"], busy: true },
  { scenario: "mixed reasons", count: 2, reasons: ["sharing violation (os error 32)", "access is denied (os error 5)"], busy: false },
  { scenario: "missing details", count: 2, reasons: ["sharing violation (os error 32)"], busy: false },
  { scenario: "unknown reason", count: 1, reasons: ["native diagnostic"], busy: false },
])("shows only supported failure guidance for $scenario", ({ count, reasons, busy }) => {
  const L = t();
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "", uninstall_ok: false,
    uninstall_message: "", deleted: 0, failed: count, skipped: 0, aborted: false, delayed: 1,
    restore_point_ok: false, restore_point_msg: "", errors: [],
    item_details: reasons.map((message, index) => ({ path: `C:\\Sample\\file-${index}.bin`, kind: "file",
      status: "failed", message })) };
  render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  expect(screen.getByText(busy ? L.reportNextBusy : L.reportNextFailed).closest("details")).toBeNull();
  expect(screen.getByText(L.reportNextDelayed).closest("details")).toBeNull();
  expect(screen.queryByText(busy ? L.reportNextFailed : L.reportNextBusy)).toBeNull();
  const failure = screen.getByText(`${L.reportFailed} · ${reasons.length}`);
  failure.closest("details")!.open = false;
  fireEvent.click(screen.getByRole("button", { name: L.reportViewFailed }));
  expect(failure.closest("details")?.open).toBe(true);
  expect(document.activeElement).toBe(failure);
});

it.each(["zh", "en"] as const)("shows the single cleanup cancellation next to its actual counts in %s", lang => {
  setLang(lang);
  const L = t();
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "",
    uninstall_ok: false, uninstall_message: "cleanup cancelled", deleted: 0, failed: 0, skipped: 0,
    aborted: true, restore_point_ok: false, restore_point_msg: "",
    errors: ["cleanup cancelled before deleting leftovers"], item_details: [] };
  render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  const notice = screen.getByRole("status");
  expect(notice.textContent).toBe(lang === "zh" ? "清理已取消。" : "Cleanup was cancelled.");
  expect(screen.getByText(`${L.cleanupResultCounts}: ${report.app_name}`)).toBeTruthy();
  expect(notice.closest("details")).toBeNull();
  expect(screen.getByRole("group", { name: L.cleanupResultCounts }).textContent).toContain(`0${L.reportDeleted}`);
  expect(screen.queryByText(L.reportProgressLabel(0, 0, 0))).toBeNull();
});

it("shows deleted, kept, and pending-reboot sizes in the report", () => {
  setLang("zh");
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "",
    uninstall_ok: true, uninstall_message: "", deleted: 1, failed: 0, skipped: 0, delayed: 1,
    reclaimed_kb: 1024, kept_kb: 2048, delayed_kb: 512, aborted: false,
    restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  const { container } = render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false}
    aiReportNote={null} verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  expect(container.textContent).toContain(t().reportReclaimedKept("1.0 MB", "2.0 MB"));
  expect(container.textContent).toContain(t().reportPendingKb("512 KB"));
});

it.each(["zh", "en"] as const)("shows the explicit sort control and backup prerequisite in %s", lang => {
  setLang(lang);
  const sortBy = vi.fn();
  render(<SoftwareToolbar q="" category="all" estimating={false} scanning={false} aiEnabled={false}
    sortCol={null} sortDesc={false} sortBy={sortBy}
    smartFilterOpen={false} onQuery={() => {}} onCategory={() => {}} onStopEstimate={() => {}}
    onOpenAi={() => {}} onToggleSmartFilter={() => {}} />);
  const choice = screen.getByRole<HTMLOptionElement>("option", { name: t().softwareSortSize });
  fireEvent.change(screen.getByRole("combobox", { name: t().softwareSort }), { target: { value: choice.value } });
  expect(sortBy).toHaveBeenCalledWith("size", true);
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
it.each(["zh", "en"] as const)("offers restoration only for a verified backup in %s", lang => {
  setLang(lang);
  const base: FullCleanupReport = { app_name: "Vendor", dry_run: false, backup_dir: "",
    uninstall_ok: true, uninstall_message: "", deleted: 1, failed: 0, skipped: 0,
    aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  const verified = { ...base, backup_dir: "session", backup_verified: true };
  for (const report of [base, { ...base, backup_dir: "session" },
    { ...base, backup_dir: "session", backup_verified: false, aborted: true },
    verified, { ...verified, aborted: true }, { ...verified, aborted: true, uninstall_ok: false },
    { ...verified, failed: 1 }, { ...verified, dry_run: true }]) {
    const view = render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false}
      aiReportNote={null} verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
    const hasRestoration = report.backup_verified === true && !report.dry_run;
    const restore = screen.queryByRole("button", { name: `${t().restorePreviewTitle}: session` });
    expect(Boolean(restore)).toBe(hasRestoration);
    if (restore) expect(restore.closest("details")).toBeNull();
    expect(screen.queryByText(t().safetyVaultBanner) !== null).toBe(hasRestoration);
    expect(Boolean(screen.queryByText(t().reportNarrative(report.deleted, report.failed, report.skipped, hasRestoration))))
      .toBe(!report.dry_run);
    expect(screen.queryByText(t().reportNextOk) !== null).toBe(hasRestoration && !report.aborted && !report.failed);
    if (!hasRestoration && !report.dry_run) {
      expect(screen.getByRole("note").textContent).toBe(report.backup_dir ? t().unverifiedBackupThisRun : t().noBackupThisRun);
    }
    view.unmount();
  }
});

it("keeps the cleanup counts, skipped reason and return visible on the report", () => {
  const L = t();
  const onDismiss = vi.fn();
  const report: FullCleanupReport = { app_name: "fixture", dry_run: false, backup_dir: "",
    uninstall_ok: false, uninstall_message: "", deleted: 0, failed: 0, skipped: 1,
    aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [],
    item_details: [{ path: "C:\\Fixture\\project", kind: "dir", status: "skipped", message: "user_data" }] };
  render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={null} onDismiss={onDismiss} onRegenerate={() => {}} />);
  expect(screen.getByRole("group", { name: L.cleanupResultCounts }).closest("details")).toBeNull();
  expect(screen.getByText(L.reportNextReason(L.reasonUserData)).closest("details")).toBeNull();
  expect(screen.getByText(L.reasonUserData).closest("details")?.open).toBe(true);
  const details = screen.getByText(L.cleanupReportDetails).closest("details")!;
  expect(details.open).toBe(true);
  details.open = false;
  fireEvent.click(screen.getByRole("button", { name: L.reportViewSkipped }));
  expect(details.open).toBe(true);
  expect(document.activeElement).toBe(screen.getByText(`${L.reportSkipped} · 1`));
  fireEvent.click(screen.getByRole("button", { name: L.navBack }));
  expect(onDismiss).toHaveBeenCalledOnce();
});

it.each(["zh", "en"] as const)("explains that unverified backup copies preserve the original in %s", lang => {
  setLang(lang);
  const report: FullCleanupReport = { app_name: "fixture", dry_run: false, backup_dir: "C:\\Backups\\fixture",
    backup_verified: false, uninstall_ok: false, uninstall_message: "", deleted: 1, failed: 0, skipped: 1,
    aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [],
    item_details: [{ path: "C:\\Fixture\\kept.bin", kind: "file", status: "skipped",
      message: "backup copy could not be verified: backup payload is missing recorded entries" }] };
  render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  expect(screen.getByRole("note").textContent).toBe(t().unverifiedBackupThisRun);
  expect(screen.queryByRole("button", { name: `${t().restorePreviewTitle}: fixture` })).toBeNull();
  expect(screen.getByText(lang === "zh" ? "备份副本无法验证，已保留原项目。" :
    "Kept the original because its backup copy could not be verified.").closest("details")?.open).toBe(true);
});

it("offers a post-cleanup rescan button and honors the busy state", () => {
  const onRescan = vi.fn();
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "",
    uninstall_ok: true, uninstall_message: "", deleted: 1, failed: 0, skipped: 0,
    aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  const view = render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false}
    aiReportNote={null} verifyRows={null} onRescan={onRescan} rescanBusy onDismiss={() => {}}
    onRegenerate={() => {}} />);
  const busy = screen.getByRole("button", { name: t().analyzing });
  expect(busy.hasAttribute("disabled")).toBe(true);
  view.rerender(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false}
    aiReportNote={null} verifyRows={null} onRescan={onRescan} rescanBusy={false}
    onDismiss={() => {}} onRegenerate={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: t().reportRescanApp }));
  expect(onRescan).toHaveBeenCalledTimes(1);
});

it("publishes the exact cleanup backup identity without executing restore", () => {
  const store = renderHook(useBackupSessionRequest);
  const report: FullCleanupReport = { app_name: "sample", dry_run: false, backup_dir: "C:\\Backups\\123_sample",
    backup_verified: true, uninstall_ok: true, uninstall_message: "", deleted: 1, failed: 0, skipped: 0,
    aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
  render(<ReportPanel report={report} aiEnabled={false} aiReportBusy={false} aiReportNote={null}
    verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: `${t().restorePreviewTitle}: 123_sample` }));
  expect(store.result.current?.name).toBe("123_sample");
  act(() => clearBackupSessionRequest(store.result.current!.id));
  expect(store.result.current).toBeNull();
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
  expect(onEvidence).toHaveBeenCalledWith(cleanupItemKey({ kind: "file", path: "fixture-path" }));
});

it.each(["zh", "en"] as const)("uses translated evidence in both leftover surfaces in %s", lang => {
  setLang(lang);
  const evidence = [{ code: "orphan_no_owner", label: "No matching uninstall entry", weight: 30,
    detail: "folder `Vendor` not matched to installed software" }];
  const item: CleanupItem = { path: "fixture-path", kind: "dir", score: 30,
    confidence: "suspected", risk: "medium", reason: "orphan", evidence };
  const onEvidence = vi.fn();
  const props = { scan: { app_name: "Vendor", items: [item] }, scanning: false,
    selectedPaths: new Set<string>(), aiNotes: {}, orphanLabel: "Orphans", onTogglePath: () => {}, onEvidence };
  const view = render(<ScanLeftoversView {...props}
    evidence={null} />);
  fireEvent.click(screen.getByRole("button", { name: `${t().orphanEvidenceTitle}: fixture-path` }));
  view.rerender(<ScanLeftoversView {...props} evidence={cleanupItemKey(item)} />);
  expect(screen.getByRole("region").textContent).toContain(`${t().orphanEvNoOwner} (30)`);
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
