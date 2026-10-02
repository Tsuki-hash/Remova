// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { setLang, t } from "../i18n";
import { ReportPanel } from "../components/ReportPanel";
import { ScanLeftoversView } from "../components/ScanLeftoversView";
import type { FullCleanupReport } from "../types";
vi.mock("@tanstack/react-virtual", () => ({ useVirtualizer: () => ({
  getVirtualItems: () => [{ index: 0, start: 0 }], getTotalSize: () => 72, measureElement: vi.fn(),
}) }));
afterEach(() => { cleanup(); setLang("zh"); });
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
