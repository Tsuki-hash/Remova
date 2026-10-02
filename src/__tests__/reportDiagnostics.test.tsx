// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { setLang, t } from "../i18n";
import { ReportPanel } from "../components/ReportPanel";
import type { FullCleanupReport } from "../types";
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
    aiReportBusy={false} aiReportNote={null} verifyRows={null} onDismiss={() => {}} onRegenerate={() => {}} />);
  expect(screen.getByText(L.backendUninstallExit("1603"))).toBeTruthy();
  expect(container.textContent).toContain(L.backendDetailUnavailable);
  const details = container.querySelector("details")!;
  expect(details.open).toBe(false);
  expect(details.textContent).toContain(report.uninstall_message);
  expect(details.textContent).toContain("native <error>");
  expect(container.querySelector("error")).toBeNull();
});
